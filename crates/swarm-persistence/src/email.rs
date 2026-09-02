use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{Task, TaskId, TaskPriority, TaskState, WorkerId};
use uuid::Uuid;

use std::fmt::Write as _;

use crate::{
    ControlRoomEventKind, EMAIL_REPLY_FROM_REVIEW_SCHEMA_VERSION, TaskStore, TaskStoreError,
    insert_control_room_event, parse_domain_id, validate_description, validate_text,
};

const MAX_SOURCE_ID_BYTES: usize = 512;
const MAX_CONVERSATION_ID_BYTES: usize = 512;
const MAX_INTERNET_MESSAGE_ID_BYTES: usize = 998;
/// A mail subject is SOURCE METADATA, not a task title, and it is bounded like
/// the other source fields rather than by the task-title limit.
///
/// Conflating the two is the bug this constant exists to end: a subject over
/// 240 bytes made `validate_email_message` refuse the whole import with "task
/// title must contain 1 to 240 bytes", naming the one field the operator could
/// see and edit and which was never the problem. They reported exactly that —
/// "the title was fine, and that shouldn't matter anyway" — and both halves
/// were right. 998 is RFC 5322's line limit, which is what actually bounds a
/// subject in transit.
const MAX_EMAIL_SUBJECT_BYTES: usize = 998;
const MAX_SENDER_NAME_BYTES: usize = 320;
const MAX_SENDER_ADDRESS_BYTES: usize = 320;
const MAX_WEB_URL_BYTES: usize = 2_048;
const MAX_EMAIL_ATTACHMENTS: usize = 16;
const MAX_EMAIL_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_EMAIL_ATTACHMENT_TOTAL_BYTES: u64 = 25 * 1024 * 1024;
const MAX_ATTACHMENT_NAME_BYTES: usize = 255;
const MAX_MEDIA_TYPE_BYTES: usize = 127;
const MAX_CONTENT_ID_BYTES: usize = 512;
pub(crate) const MAX_DEPLOYMENT_FIELD_BYTES: usize = 512;
const MAX_EMAIL_REPLY_BYTES: usize = 10_000;
const MAX_PENDING_EMAIL_REPLIES: i64 = 256;
const MAX_EMAIL_MESSAGES_PER_TASK: usize = 20;

#[derive(Clone, Copy, Debug)]
pub struct EmailAttachmentSnapshot<'a> {
    pub storage_name: &'a str,
    pub display_name: &'a str,
    pub media_type: &'a str,
    pub byte_size: u64,
    pub inline: bool,
    pub content_id: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub struct EmailMessageSnapshot<'a> {
    pub integration_id: &'a str,
    pub message_id: &'a str,
    pub conversation_id: &'a str,
    pub internet_message_id: Option<&'a str>,
    pub subject: &'a str,
    pub sender_name: &'a str,
    pub sender_address: &'a str,
    pub received_at: i64,
    pub web_url: &'a str,
    /// Sanitized, readable plain text. HTML and remote content are adapter concerns.
    pub body_text: &'a str,
    pub attachments: &'a [EmailAttachmentSnapshot<'a>],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmailTaskAttachment {
    pub storage_name: String,
    pub display_name: String,
    pub media_type: String,
    pub byte_size: u64,
    pub inline: bool,
    pub content_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmailTaskLink {
    pub id: String,
    pub task_id: TaskId,
    pub integration_id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub internet_message_id: Option<String>,
    pub sender_name: String,
    pub sender_address: String,
    pub received_at: i64,
    pub web_url: String,
    pub imported_at: i64,
    pub attachments: Vec<EmailTaskAttachment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmailImport {
    pub task: Task,
    pub source: EmailTaskLink,
    pub sources: Vec<EmailTaskLink>,
    pub created: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct EmailTaskDraft<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub priority: TaskPriority,
    pub worker_id: Option<WorkerId>,
    pub state: TaskState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskDeploymentRecord {
    pub id: String,
    pub task_id: TaskId,
    pub environment: String,
    pub reference: String,
    pub deployed_at: i64,
    pub recorded_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailReplyState {
    Draft,
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
    Cancelled,
}

impl std::fmt::Display for EmailReplyState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Draft => "draft",
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
            Self::Cancelled => "cancelled",
        })
    }
}

impl FromStr for EmailReplyState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// A completed email task whose requester has not been answered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UnansweredEmailTask {
    pub task_id: TaskId,
    pub title: String,
    pub sender_name: String,
    pub sender_address: String,
    /// When the earliest message in the thread arrived.
    pub received_at: i64,
    /// This reply is ON ITS WAY — queued or dispatching, not waiting on anyone.
    ///
    /// Without it a reply mid-flight reads exactly like one nobody wrote: the
    /// body lives on a row in state 'queued', and a query looking only for
    /// 'draft' finds nothing. The operator sent seven threads at once on
    /// 2026-08-26, watched Sent Items fill up, and the queue told them no reply
    /// had been written for the ones still going out.
    pub sending: bool,
    /// A reply exists but was never sent. Writing one is not sending it.
    pub drafted: bool,
    /// The drafted reply itself, so the operator can read and send it without
    /// going and finding the task. Reviewing the words is the only part of
    /// this that is theirs; the worker verifies that the work is running.
    pub draft_id: Option<String>,
    pub draft_body: Option<String>,
    /// The worker that carried this work, so the queue says whose it was
    /// rather than only that something is waiting.
    pub worker_name: Option<String>,
    /// Why the last attempt to deliver this reply failed, when one did.
    ///
    /// A cancelled reply is terminal and used to be hidden entirely, so a send
    /// that never left the building was indistinguishable from one that did.
    pub delivery_failure: Option<String>,
    /// How many original threads one send actually answers.
    ///
    /// A task can be linked to several inbound emails, and the reply fans out
    /// to every one of them. The queue named only the earliest sender, so a
    /// send that reached seven people looked exactly like a send that reached
    /// one — and the seven-target case is not hypothetical, it is in this
    /// Hive's own history. Deciding whether to press Send without knowing how
    /// many people hear about it is not a decision.
    pub thread_count: usize,
    /// Why no reply exists, when the board knows.
    ///
    /// The operator was shown an empty textarea on a completed task and read it
    /// as a request: "It wanted me to write it? that isn't how this should be
    /// working." It was not a request — a worker HAD written a reply and the
    /// gate refused it, and that refusal died with the worker's turn. Now it is
    /// recorded on the task, and the card says it out loud.
    pub no_reply_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmailReplyDispatch {
    pub id: String,
    pub task_id: TaskId,
    pub body: String,
    pub state: EmailReplyState,
    pub idempotency_key: String,
    pub attempts: u8,
    pub available_at: i64,
    pub attempted_at: Option<i64>,
    pub delivered_at: Option<i64>,
    pub provider_reply_id: Option<String>,
    pub last_error: Option<String>,
    pub targets: Vec<EmailReplyTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmailReplyTarget {
    pub id: String,
    pub source_id: String,
    pub sender_name: String,
    pub sender_address: String,
    pub web_url: String,
    pub state: EmailReplyState,
    pub attempts: u8,
    pub attempted_at: Option<i64>,
    pub delivered_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailReplyTargetDispatch {
    pub target_id: String,
    pub reply_id: String,
    pub task_id: TaskId,
    pub message_id: String,
    pub body: String,
    pub attempts: u8,
    /// The stable RFC 5322 Message-ID. A Graph `id` is folder-scoped and
    /// changes when a message moves; this does not, so it is what finds the
    /// message again when the stored id has gone stale.
    pub internet_message_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmailReplyFailure {
    Retryable(String),
    Uncertain(String),
    Permanent(String),
}

impl TaskStore {
    /// Moves crash-interrupted reply sends to explicit uncertainty. They are never
    /// replayed automatically because Microsoft may have accepted the original send.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn recover_inflight_email_replies(&self) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE email_reply_targets
             SET state = 'uncertain', last_error = 'Swarm restarted before delivery was confirmed',
                 updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?;
        connection.execute(
            "UPDATE email_reply_deliveries SET state = 'uncertain',
                 last_error = 'Swarm restarted before delivery was confirmed', updated_at = unixepoch()
             WHERE id IN (SELECT reply_id FROM email_reply_targets WHERE state = 'uncertain')",
            [],
        )?;
        Ok(changed)
    }

    /// Imports one already-sanitized message and its private attachment metadata atomically.
    /// Repeating the exact integration/message identity returns the original task.
    ///
    /// # Errors
    /// Rejects invalid or oversized source data and unavailable persistence.
    pub fn import_email_message(
        &self,
        message: &EmailMessageSnapshot<'_>,
        priority: TaskPriority,
    ) -> Result<EmailImport, TaskStoreError> {
        let title = email_task_title(message);
        self.import_email_messages(
            std::slice::from_ref(message),
            &EmailTaskDraft {
                title: &title,
                description: message.body_text.trim(),
                priority,
                worker_id: None,
                state: TaskState::Draft,
            },
        )
    }

    /// Imports one or more immutable email threads into one task atomically.
    /// Every source remains independently addressable for attachments and linked context.
    ///
    /// # Errors
    /// Rejects invalid content, mixed existing ownership, unsupported initial states,
    /// unknown workers, exhausted dispatch capacity, or unavailable persistence.
    pub fn import_email_messages(
        &self,
        messages: &[EmailMessageSnapshot<'_>],
        draft: &EmailTaskDraft<'_>,
    ) -> Result<EmailImport, TaskStoreError> {
        if messages.is_empty() || messages.len() > MAX_EMAIL_MESSAGES_PER_TASK {
            return Err(TaskStoreError::InvalidEmailMessage);
        }
        for message in messages {
            validate_email_message(message)?;
        }
        let title = draft.title.trim();
        let description = draft.description.trim();
        validate_text(title, "email://inbox")?;
        validate_description(description)?;
        if !matches!(draft.state, TaskState::Draft | TaskState::Ready) {
            return Err(TaskStoreError::InvalidTransition {
                from: TaskState::Draft,
                to: draft.state,
            });
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(task_id) = existing_email_task_id(&transaction, messages)? {
            transaction.commit()?;
            drop(connection);
            let sources = self.email_task_links_for_task(task_id)?;
            let source = sources
                .first()
                .cloned()
                .ok_or(TaskStoreError::EmailSourceNotFound)?;
            return Ok(EmailImport {
                task: self.get_task(task_id)?,
                source,
                sources,
                created: false,
            });
        }

        let task_id = insert_email_import_task(&transaction, messages, draft, title, description)?;
        insert_email_sources(&transaction, task_id, messages)?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        let sources = self.email_task_links_for_task(task_id)?;
        let source = sources
            .first()
            .cloned()
            .ok_or(TaskStoreError::EmailSourceNotFound)?;
        Ok(EmailImport {
            task: self.get_task(task_id)?,
            source,
            sources,
            created: true,
        })
    }

    /// Returns the immutable email source attached to a task, without credentials.
    ///
    /// # Errors
    /// Returns an error when source metadata is corrupt or persistence is unavailable.
    pub fn email_task_link(
        &self,
        task_id: TaskId,
    ) -> Result<Option<EmailTaskLink>, TaskStoreError> {
        Ok(self.email_task_links_for_task(task_id)?.into_iter().next())
    }

    /// Returns every immutable source thread attached to one task.
    ///
    /// # Errors
    /// Returns an error when source metadata is corrupt or persistence is unavailable.
    pub fn email_task_links_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Vec<EmailTaskLink>, TaskStoreError> {
        let connection = self.connection()?;
        let mut sources = {
            let mut statement = connection.prepare(
                "SELECT id, task_id, integration_id, message_id, conversation_id,
                        internet_message_id, sender_name, sender_address, received_at,
                        web_url, imported_at
                 FROM email_message_links WHERE task_id = ?1
                 ORDER BY received_at, imported_at, id",
            )?;
            statement
                .query_map([task_id.to_string()], email_task_link_from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut statement = connection.prepare(
            "SELECT storage_name, display_name, media_type, byte_size, is_inline, content_id
             FROM email_task_attachments WHERE source_id = ?1 ORDER BY created_at, id",
        )?;
        for source in &mut sources {
            source.attachments = statement
                .query_map([&source.id], email_attachment_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(sources)
    }

    /// Lists immutable email sources attached to tasks, newest imports first.
    ///
    /// # Errors
    /// Returns an error when source metadata is corrupt or persistence is unavailable.
    pub fn email_task_links(&self) -> Result<Vec<EmailTaskLink>, TaskStoreError> {
        let connection = self.connection()?;
        let mut sources = {
            let mut statement = connection.prepare(
                "SELECT id, task_id, integration_id, message_id, conversation_id,
                        internet_message_id, sender_name, sender_address, received_at,
                        web_url, imported_at
                 FROM email_message_links ORDER BY imported_at DESC, task_id DESC",
            )?;
            statement
                .query_map([], email_task_link_from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut attachment_statement = connection.prepare(
            "SELECT storage_name, display_name, media_type, byte_size, is_inline, content_id
             FROM email_task_attachments WHERE source_id = ?1 ORDER BY created_at, id",
        )?;
        for source in &mut sources {
            source.attachments = attachment_statement
                .query_map([&source.id], email_attachment_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(sources)
    }

    /// Lists recorded deployments for one task, newest first.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn task_deployments(
        &self,
        task_id: TaskId,
    ) -> Result<Vec<TaskDeploymentRecord>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, task_id, environment, reference, deployed_at, recorded_at
             FROM task_deployments WHERE task_id = ?1
             ORDER BY deployed_at DESC, recorded_at DESC, id DESC",
        )?;
        Ok(statement
            .query_map([task_id.to_string()], deployment_from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Returns the one reply lifecycle record attached to an email task.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn email_reply_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Option<EmailReplyDispatch>, TaskStoreError> {
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "SELECT id FROM email_reply_deliveries WHERE task_id = ?1",
                [task_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        id.map_or(Ok(None), |id| email_reply_by_id(&connection, &id))
    }

    /// Completed email tasks whose thread nobody ever answered.
    ///
    /// A task imported from email carries a person waiting on it. Finishing the
    /// work does not tell them anything: the reply is a separate, deliberate
    /// step, and until it is sent the requester has heard nothing. Nothing
    /// noticed that silence, so a completed task simply went quiet.
    ///
    /// A draft that exists but was never sent still counts as unanswered —
    /// writing a reply is not sending one.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or holds an invalid ID.
    /// Finished work whose requester was never answered, with whatever reply
    /// has been written for them.
    ///
    /// THE BODY OF A CANCELLED REPLY IS STILL THE REPLY. This once read the
    /// body only from a row in state 'draft', and a send that fails leaves the
    /// row in 'cancelled' — body and all. So the queue announced that no reply
    /// had been written while four of them, 1.0 to 1.6 KB each, sat unreadable
    /// in the database on 2026-08-25, and pressing Write the reply opened an
    /// empty box over the top of them. The operator reported generating all the
    /// emails, watching them fail, and the drafts appearing to be lost. They
    /// were never lost.
    ///
    /// Worth noticing what the old query did: it read `last_error` off the
    /// cancelled row and the body from nowhere. It looked straight at the draft
    /// and took only the bad news.
    ///
    /// A draft still wins where one exists, because that is the newer text; the
    /// cancelled body is the fallback rather than the preference.
    ///
    /// # Errors
    /// Returns a persistence error when the queue cannot be read.
    pub fn completed_email_tasks_awaiting_a_reply(
        &self,
    ) -> Result<Vec<UnansweredEmailTask>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT t.id, t.title, link.sender_name, link.sender_address,
                    MIN(link.received_at),
                    EXISTS(SELECT 1 FROM email_reply_deliveries r
                           WHERE r.task_id = t.id
                             AND r.state IN ('draft', 'cancelled', 'queued', 'dispatching')
                             AND trim(r.body) <> ''),
                    EXISTS(SELECT 1 FROM email_reply_deliveries r
                           WHERE r.task_id = t.id
                             AND r.state IN ('queued', 'dispatching')),
                    (SELECT r.id FROM email_reply_deliveries r
                     WHERE r.task_id = t.id AND r.state = 'draft'
                     ORDER BY r.created_at DESC, r.id DESC LIMIT 1),
                    -- A cancelled reply still holds its body; see the doc
                    -- comment. A draft wins when one exists.
                    COALESCE(
                        (SELECT r.body FROM email_reply_deliveries r
                         WHERE r.task_id = t.id AND r.state = 'draft'
                         ORDER BY r.created_at DESC, r.id DESC LIMIT 1),
                        (SELECT r.body FROM email_reply_deliveries r
                         WHERE r.task_id = t.id
                           AND r.state IN ('cancelled', 'queued', 'dispatching')
                           AND trim(r.body) <> ''
                         ORDER BY r.updated_at DESC, r.id DESC LIMIT 1)
                    ),
                    (SELECT w.name FROM worker_profiles w WHERE w.id = t.assigned_worker_id),
                    COUNT(link.id),
                    (SELECT r.last_error FROM email_reply_deliveries r
                     WHERE r.task_id = t.id AND r.state = 'cancelled'
                     ORDER BY r.updated_at DESC, r.id DESC LIMIT 1),
                    -- WHY NOBODY WROTE ONE, when nobody did. Saying only that
                    -- no reply has been written is accurate and useless: it
                    -- tells the operator the box is empty, which they can see,
                    -- and leaves them to conclude the writing is theirs.
                    (SELECT a.note FROM task_activity a
                     WHERE a.task_id = t.id AND a.kind = 'noted'
                       AND a.note LIKE '%email reply%refused%'
                     ORDER BY a.sequence DESC LIMIT 1)
             FROM tasks t
             JOIN email_message_links link ON link.task_id = t.id
             WHERE t.state = 'completed' AND t.removed_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM email_reply_deliveries reply
                   WHERE reply.task_id = t.id AND reply.state = 'delivered'
               )
               -- A CANCELLED REPLY IS SHOWN, carrying why it failed. This used
               -- to exclude them, for a reason that was sound and rested on a
               -- premise that was wrong -- that the operator had deleted
               -- the source messages, so sending reported not found.
               --
               -- Not found did not mean deleted. A Graph id is folder-scoped and
               -- changes when a message MOVES, so filing an email quietly made
               -- it unanswerable. Seventeen replies were cancelled that way on
               -- 2026-08-25 and this clause hid every one: the operator pressed
               -- Send, the item left the queue, and it looked handled. They
               -- found out by opening Outlook and seeing nothing.
               --
               -- The original worry — an item nobody can ever clear — is
               -- answered by the resolver rather than by hiding it: a stale id
               -- is now looked up again by its stable internet id, so a retry
               -- can actually succeed. When the message really is gone, the
               -- card says so and the operator can dismiss it knowing why.
             GROUP BY t.id
             ORDER BY MIN(link.received_at), t.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(unanswered_email_task).collect()
    }

    /// Records operator-approved deployment evidence for a completed task.
    ///
    /// # Errors
    /// Rejects incomplete work, invalid evidence, or unavailable persistence.
    pub fn record_task_deployment(
        &self,
        task_id: TaskId,
        environment: &str,
        reference: &str,
        deployed_at: i64,
    ) -> Result<TaskDeploymentRecord, TaskStoreError> {
        self.record_deployment_delivering(task_id, environment, reference, deployed_at, true)
    }

    /// Records that PART of this task shipped, without claiming the task is done.
    ///
    /// The deterministic sweep closes work in Review on a deployment record,
    /// because a deployment normally is the completion evidence. Partial
    /// delivery is common and there was no way to say it: on 2026-09-02 a
    /// worker moved B7 to Review saying in capitals that two of three
    /// acceptance lines were unmet, recorded a true deployment for the half
    /// that had shipped, and the sweep closed the ticket one second later.
    ///
    /// This is not the reviewer's-hold case. A hold is an OPINION the sweep was
    /// ruled to override; this is the EVIDENCE declaring its own scope.
    ///
    /// # Errors
    /// Same as [`Self::record_task_deployment`].
    pub fn record_partial_task_deployment(
        &self,
        task_id: TaskId,
        environment: &str,
        reference: &str,
        deployed_at: i64,
    ) -> Result<TaskDeploymentRecord, TaskStoreError> {
        self.record_deployment_delivering(task_id, environment, reference, deployed_at, false)
    }

    fn record_deployment_delivering(
        &self,
        task_id: TaskId,
        environment: &str,
        reference: &str,
        deployed_at: i64,
        delivers_whole_task: bool,
    ) -> Result<TaskDeploymentRecord, TaskStoreError> {
        let environment = environment.trim();
        let reference = reference.trim();
        if !bounded_text(environment, MAX_DEPLOYMENT_FIELD_BYTES)
            || !bounded_text(reference, MAX_DEPLOYMENT_FIELD_BYTES)
            || deployed_at <= 0
        {
            return Err(TaskStoreError::InvalidTaskDeployment {
                max: MAX_DEPLOYMENT_FIELD_BYTES,
            });
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task_state = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                [task_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        // Review counts as well as completed. A worker can only report Active,
        // Blocked or Review for its own work, so requiring completion here made
        // this unreachable by the one actor that knows where the work is
        // running — and the briefing asks that actor to record it. Review is
        // exactly the moment it knows: finished, handed off, still on the task.
        if task_state != TaskState::Completed.to_string()
            && task_state != TaskState::Review.to_string()
            && task_state != TaskState::AwaitingRelease.to_string()
        {
            // Says which rule was broken. "evidence is invalid" is what this
            // used to answer for a perfectly well-formed reference recorded a
            // moment too early, and it cost two failed attempts and a bug
            // report filed against the wrong thing entirely.
            return Err(TaskStoreError::DeploymentEvidenceTooEarly);
        }
        let id = Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT OR IGNORE INTO task_deployments (
                 id, task_id, environment, reference, deployed_at, approved_by_operator_id,
                 delivers_whole_task
             ) SELECT ?1, ?2, ?3, ?4, ?5, o.id, ?6
               FROM local_hive_identity local
               JOIN hives h ON h.id = local.hive_id
               JOIN operators o ON o.id = h.operator_id
               WHERE local.singleton = 1",
            params![
                id,
                task_id.to_string(),
                environment,
                reference,
                deployed_at,
                i64::from(delivers_whole_task)
            ],
        )?;
        let record = transaction.query_row(
            "SELECT id, task_id, environment, reference, deployed_at, recorded_at
             FROM task_deployments
             WHERE task_id = ?1 AND environment = ?2 AND reference = ?3",
            params![task_id.to_string(), environment, reference],
            deployment_from_row,
        )?;
        // AWAITING RELEASE SETTLES ITSELF HERE, and this is the whole point of
        // the state. Work parked waiting to ship has already been accepted; the
        // only open question was whether it shipped, and this call answers it.
        //
        // NOTHING WAITS ON A PERSON for a question a fact has just settled. The
        // completion gate is satisfied by exactly this evidence, so asking
        // somebody to click afterwards would be asking them to agree with a
        // deployment record they are looking at.
        //
        // Only from AwaitingRelease. A deployment recorded against work still in
        // Review does NOT close it: that work has not been accepted yet, and
        // shipping something is not the same as somebody agreeing it is done.
        if task_state == TaskState::AwaitingRelease.to_string() && delivers_whole_task {
            transaction.execute(
                "UPDATE tasks SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    task_id.to_string(),
                    TaskState::Completed.to_string(),
                    deployed_at
                ],
            )?;
            transaction.execute(
                "INSERT INTO task_activity (task_id, kind, from_state, to_state, note, actor_kind)
                 VALUES (?1, 'state_changed', ?2, ?3, ?4, 'system')",
                params![
                    task_id.to_string(),
                    TaskState::AwaitingRelease.to_string(),
                    TaskState::Completed.to_string(),
                    format!("Released to {environment} at {reference}."),
                ],
            )?;
        }
        // A DEPLOYMENT CONTRADICTS AN UNAPPROVED CLAIM THAT NOTHING SHIPPED,
        // and the contradiction is knowable here rather than by someone
        // stumbling on it later.
        //
        // Five tasks on this board carry both right now, none ever looked at.
        // The sharpest still says "PR #418 is open" for work that merged and
        // deployed — and that task was later cited as the gate for the step
        // after it. The record says nothing shipped.
        //
        // Superseding is a statement of fact, not a judgement: the claim was
        // that nothing shipped, and something did. It is deliberately narrow —
        // an APPROVED exemption is untouched, because Queen accepted that
        // argument and quietly rewriting an accepted decision is a different
        // and worse act than leaving a stale claim standing.
        //
        // The reason is preserved and prefixed rather than replaced. It was
        // true when written, and what it said is how anyone later understands
        // why the task looked finished without shipping anything.
        transaction.execute(
            "UPDATE task_completion_exemptions
                SET reason = 'SUPERSEDED by a recorded deployment. The claim below was true when it was made:'
                             || char(10) || reason,
                    superseded_at = ?2
              WHERE task_id = ?1 AND approved_at IS NULL AND superseded_at IS NULL",
            params![task_id.to_string(), deployed_at],
        )?;
        transaction.commit()?;
        Ok(record)
    }

    /// Creates one reviewed reply draft once the linked work is deployed and
    /// either finished or handed to review.
    ///
    /// Review is included because a worker cannot mark its own task completed,
    /// so gating on completion alone left the reply to be written by whoever
    /// closed the task later — which is how the person who wrote in ended up
    /// waiting while the operator drafted it by hand. This still only drafts.
    ///
    /// # Errors
    /// Rejects ineligible work, duplicate or invalid replies, and a full bounded queue.
    pub fn prepare_email_reply(
        &self,
        task_id: TaskId,
        body: &str,
    ) -> Result<EmailReplyDispatch, TaskStoreError> {
        let body = body.trim();
        if !bounded_text(body, MAX_EMAIL_REPLY_BYTES) {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // WRITING IS NOT SENDING, and this used to conflate them.
        //
        // The evidence test — deployment or approved exemption — lived HERE, on
        // the draft, while queue_email_reply, which actually puts words in
        // somebody's inbox, tested `state = 'draft'` and nothing else. So the
        // rule everyone believed was being enforced was guarding a textarea and
        // not the send.
        //
        // It also could not be satisfied. The evidence is approved by Queen
        // AFTER the worker's turn ends, so the one window in which a worker
        // could write the reply was a window it usually did not have. The
        // operator got a blank box with "No reply has been written" and became
        // the author of a reply a worker was supposed to write: "It wanted me to
        // write it? that isn't how this should be working."
        //
        // So drafting now asks only that the work is done being worked —
        // review or completed — and the evidence test moved to the send, where
        // it is enforced for the first time. This is not a loosening. Nothing
        // reaches a person any earlier than it did before.
        // ASKED AS TWO QUESTIONS, because they have different answers and one
        // message for both names the wrong precondition.
        //
        // This used to be a single EXISTS with a JOIN onto email_message_links,
        // so a task with no email thread at all reported itself as "this task is
        // neither in review nor completed" — while sitting in completed. That is
        // the same shape as the subject-length error that told an operator their
        // task title was wrong when it was the email SUBJECT being validated,
        // and the deployment message that sent two sessions away believing they
        // had merely arrived too early. A refusal that names the wrong field
        // sends the reader to the wrong place, and they can be there a while.
        let has_thread: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM email_message_links WHERE task_id = ?1)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if !has_thread {
            return Err(TaskStoreError::TaskHasNoEmailThread);
        }
        let ready: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 WHERE task.id = ?1 AND task.state IN ('completed', 'review')
             )",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if !ready {
            // A REFUSAL THAT DIES WITH THE TURN IS INVISIBLE. That is the other
            // half of this defect: a worker hit the old gate, its turn ended,
            // and nothing on the board recorded that a reply had been attempted
            // at all. Commit the trace before returning the error, so the next
            // reader sees it instead of an unexplained empty box.
            transaction.execute(
                "INSERT INTO task_activity (task_id, kind, note, actor_kind)
                 VALUES (?1, 'noted', ?2, 'system')",
                params![
                    task_id.to_string(),
                    "An email reply was written and refused: a reply can be drafted once the task \
                     is in review or completed, and this task is neither."
                ],
            )?;
            transaction.commit()?;
            return Err(TaskStoreError::EmailDraftNotReady);
        }
        let queued: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM email_reply_deliveries
             WHERE state IN ('draft','queued','dispatching')",
            [],
            |row| row.get(0),
        )?;
        if queued >= MAX_PENDING_EMAIL_REPLIES {
            return Err(TaskStoreError::EmailReplyQueueFull);
        }
        // One reply row per task, so a cancelled one blocks every future
        // attempt until it is cleared. It was reachable from the UI in exactly
        // one direction: into the dead end. Clearing it lets the operator write
        // again if the thread comes back, and costs nothing if it does not.
        transaction.execute(
            "DELETE FROM email_reply_deliveries WHERE task_id = ?1 AND state = 'cancelled'",
            [task_id.to_string()],
        )?;
        let id = Uuid::now_v7().to_string();
        let idempotency_key = format!("email-resolution:{task_id}");
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO email_reply_deliveries (
                 id, task_id, body, state, idempotency_key
             ) VALUES (?1, ?2, ?3, 'draft', ?4)",
            params![id, task_id.to_string(), body, idempotency_key],
        )?;
        if inserted != 1 {
            return Err(TaskStoreError::EmailReplyAlreadyExists);
        }
        let dispatch = transaction.query_row(
            "SELECT id, task_id, body, state, idempotency_key, attempts,
                    available_at, attempted_at, delivered_at, provider_reply_id, last_error
             FROM email_reply_deliveries WHERE id = ?1",
            [&id],
            email_reply_from_row,
        )?;
        // One reply per person, on the thread they wrote in most recently.
        //
        // Every linked message used to become a target, so a task merged from
        // five messages by one person offered to send that person five
        // identical emails. Merging is what the operator did to make it one
        // piece of work; answering it five times undoes that where it is most
        // visible — in their inbox.
        //
        // Keyed by sender address rather than by thread, so a task merged from
        // several people still answers each of them, once.
        let source_ids = {
            let mut statement = transaction.prepare(
                "SELECT id FROM email_message_links source
                 WHERE source.task_id = ?1
                   AND source.id = (
                       SELECT newest.id FROM email_message_links newest
                       WHERE newest.task_id = source.task_id
                         AND newest.sender_address = source.sender_address
                       ORDER BY newest.received_at DESC, newest.imported_at DESC, newest.id DESC
                       LIMIT 1
                   )
                 ORDER BY source.received_at, source.imported_at, source.id",
            )?;
            statement
                .query_map([task_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for source_id in source_ids {
            transaction.execute(
                "INSERT INTO email_reply_targets (id, reply_id, source_id, state)
                 VALUES (?1, ?2, ?3, 'draft')",
                params![Uuid::now_v7().to_string(), id, source_id],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.email_reply_for_task(task_id)?.ok_or_else(|| {
            TaskStoreError::IntegrityFailure(format!("reply {} disappeared", dispatch.id))
        })
    }

    /// Updates a reply while it is still an operator-reviewed draft.
    ///
    /// # Errors
    /// Rejects invalid text, unknown replies, replies already queued for delivery, or unavailable persistence.
    pub fn update_email_reply_draft(
        &self,
        task_id: TaskId,
        body: &str,
    ) -> Result<EmailReplyDispatch, TaskStoreError> {
        let body = body.trim();
        if !bounded_text(body, MAX_EMAIL_REPLY_BYTES) {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE email_reply_deliveries SET body = ?2, updated_at = unixepoch()
             WHERE task_id = ?1 AND state = 'draft'",
            params![task_id.to_string(), body],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        drop(connection);
        self.email_reply_for_task(task_id)?
            .ok_or(TaskStoreError::InvalidEmailReply)
    }

    /// Moves an operator-reviewed draft into the durable delivery queue.
    ///
    /// # Errors
    /// Rejects unknown or non-draft replies and unavailable persistence.
    pub fn queue_email_reply(&self, id: &str) -> Result<EmailReplyDispatch, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // THE EVIDENCE TEST LIVES HERE NOW, and this is the first time it has
        // guarded anything that reaches a person. It was on the draft, which is
        // private and reversible; sending is neither.
        //
        // A DEPLOYMENT OR AN APPROVED EXEMPTION — the same evidence that closes
        // the task. Requiring a deployment specifically made work that
        // legitimately shipped nothing permanently unanswerable: a worker
        // established that a question needed no change, Queen approved the
        // exemption, the worker noted "Answer drafted", and there was nowhere to
        // put it. Approved only; a claim nobody has approved is not evidence.
        let ready: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM email_reply_deliveries reply
                 JOIN tasks task ON task.id = reply.task_id
                 WHERE reply.id = ?1 AND task.state IN ('completed', 'review')
                   AND (
                       EXISTS (SELECT 1 FROM task_deployments deployment
                                WHERE deployment.task_id = task.id)
                       OR EXISTS (SELECT 1 FROM task_completion_exemptions exemption
                                   WHERE exemption.task_id = task.id
                                     AND exemption.approved_at IS NOT NULL)
                   )
             )",
            [id],
            |row| row.get(0),
        )?;
        if !ready {
            return Err(TaskStoreError::EmailReplyNotReady);
        }
        let changed = transaction.execute(
            "UPDATE email_reply_deliveries SET state = 'queued', updated_at = unixepoch()
             WHERE id = ?1 AND state = 'draft'",
            [id],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        transaction.execute(
            "UPDATE email_reply_targets SET state = 'queued', available_at = unixepoch(), updated_at = unixepoch()
             WHERE reply_id = ?1 AND state = 'draft'",
            [id],
        )?;
        let reply = refresh_email_reply_summary(&transaction, id)?;
        transaction.commit()?;
        Ok(reply)
    }

    /// Claims the oldest due reply for the adapter. Credentials never enter this record.
    ///
    /// # Errors
    /// Returns an error when queue state is corrupt or persistence is unavailable.
    pub fn claim_email_reply(&self) -> Result<Option<EmailReplyTargetDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = transaction
            .query_row(
                "SELECT id FROM email_reply_targets
                 WHERE state = 'queued' AND available_at <= unixepoch()
                   AND attempts < 3
                 ORDER BY available_at, created_at, id LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(id) = id else {
            return Ok(None);
        };
        transaction.execute(
            "UPDATE email_reply_targets
             SET state = 'dispatching', attempts = attempts + 1,
                 attempted_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?1",
            [&id],
        )?;
        let dispatch = transaction.query_row(
            "SELECT target.id, reply.id, reply.task_id, source.message_id, reply.body,
                    target.attempts, source.internet_message_id
               FROM email_reply_targets target
               JOIN email_reply_deliveries reply ON reply.id = target.reply_id
               JOIN email_message_links source ON source.id = target.source_id
              WHERE target.id = ?1",
            [&id],
            |row| {
                Ok(EmailReplyTargetDispatch {
                    target_id: row.get(0)?,
                    reply_id: row.get(1)?,
                    task_id: parse_domain_id::<TaskId>(&row.get::<_, String>(2)?)?,
                    message_id: row.get(3)?,
                    body: row.get(4)?,
                    attempts: row.get(5)?,
                    internet_message_id: row.get(6)?,
                })
            },
        )?;
        refresh_email_reply_summary(&transaction, &dispatch.reply_id)?;
        transaction.commit()?;
        Ok(Some(dispatch))
    }

    /// Marks one claimed reply delivered with the provider's opaque receipt identity.
    ///
    /// # Errors
    /// Rejects invalid receipts, non-dispatching work, and unavailable persistence.
    pub fn complete_email_reply(
        &self,
        id: &str,
        provider_reply_id: &str,
    ) -> Result<EmailReplyDispatch, TaskStoreError> {
        if !bounded_text(provider_reply_id.trim(), MAX_SOURCE_ID_BYTES) {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        let connection = self.connection()?;
        let target_id = resolve_email_reply_target_id(&connection, id, "dispatching")?;
        let changed = connection.execute(
            "UPDATE email_reply_targets
             SET state = 'delivered', provider_reply_id = ?2, delivered_at = unixepoch(),
                 last_error = NULL, updated_at = unixepoch()
             WHERE id = ?1 AND state = 'dispatching'",
            params![target_id, provider_reply_id.trim()],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        reply_for_target(&connection, &target_id)
    }

    /// Explicitly retries a crash-ambiguous reply after operator review.
    ///
    /// # Errors
    /// Rejects unknown or non-uncertain replies and unavailable persistence.
    pub fn retry_uncertain_email_reply(
        &self,
        id: &str,
    ) -> Result<EmailReplyDispatch, TaskStoreError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE email_reply_targets
             SET state = 'queued', available_at = unixepoch(),
                 last_error = NULL, updated_at = unixepoch()
             WHERE reply_id = ?1 AND state = 'uncertain' AND attempts < 3",
            [id],
        )?;
        if changed == 0 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        refresh_email_reply_summary(&connection, id)
    }

    /// Returns a failed claim to a bounded retry or terminal state.
    ///
    /// # Errors
    /// Rejects invalid failure evidence, non-dispatching work, and unavailable persistence.
    pub fn fail_email_reply(
        &self,
        id: &str,
        failure: &EmailReplyFailure,
    ) -> Result<EmailReplyDispatch, TaskStoreError> {
        let (state, error, retry_delay) = match failure {
            EmailReplyFailure::Retryable(error) => ("queued", error, 30),
            EmailReplyFailure::Uncertain(error) => ("uncertain", error, 300),
            EmailReplyFailure::Permanent(error) => ("cancelled", error, 0),
        };
        let error = error.trim();
        if error.is_empty() || error.len() > 1_000 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        let connection = self.connection()?;
        let target_id = resolve_email_reply_target_id(&connection, id, "dispatching")?;
        let attempts = connection
            .query_row(
                "SELECT attempts FROM email_reply_targets
                 WHERE id = ?1 AND state = 'dispatching'",
                [&target_id],
                |row| row.get::<_, u8>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidEmailReply)?;
        let state = if state == "queued" && attempts >= 3 {
            "cancelled"
        } else {
            state
        };
        let changed = connection.execute(
            "UPDATE email_reply_targets
             SET state = ?2, available_at = unixepoch() + ?3,
                 last_error = ?4, updated_at = unixepoch()
             WHERE id = ?1 AND state = 'dispatching'",
            params![target_id, state, retry_delay, error],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        reply_for_target(&connection, &target_id)
    }
}

fn existing_email_task_id(
    transaction: &rusqlite::Transaction<'_>,
    messages: &[EmailMessageSnapshot<'_>],
) -> Result<Option<TaskId>, TaskStoreError> {
    let mut task_ids = Vec::new();
    let mut linked_count = 0_usize;
    let mut held = Vec::new();
    for message in messages {
        // JOINED TO tasks, and a REMOVED task does not count as holding the
        // message. Without this a task removed by mistake kept its emails
        // forever: removal is a soft delete, so the link outlived it and there
        // was no route anywhere in the product to break it. The remedy for a
        // wrong import is to remove the task it made, and that has to work.
        if let Some((task_id, title)) = transaction
            .query_row(
                "SELECT link.task_id, task.title
                 FROM email_message_links link
                 JOIN tasks task ON task.id = link.task_id AND task.removed_at IS NULL
                 WHERE link.integration_id = ?1 AND link.message_id = ?2",
                params![message.integration_id.trim(), message.message_id.trim()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            linked_count += 1;
            held.push((email_task_title(message), title));
            task_ids.push(task_id);
        }
    }
    task_ids.sort();
    task_ids.dedup();
    if task_ids.len() > 1 || (!task_ids.is_empty() && linked_count != messages.len()) {
        // NAMES THEM. The bare sentence was accurate and unactionable: it said a
        // selection conflicted without saying which of six messages did it, so
        // the only way forward was to deselect one at a time.
        let mut detail = format!(
            "{linked_count} of the {} selected messages are already attached to a task",
            messages.len()
        );
        for (subject, task_title) in held.iter().take(4) {
            let _ = write!(detail, "\n  \"{subject}\" is on \"{task_title}\"");
        }
        if held.len() > 4 {
            let _ = write!(detail, "\n  and {} more", held.len() - 4);
        }
        detail.push_str("\nDeselect those, or remove the task holding them to free the messages.");
        return Err(TaskStoreError::EmailMergeConflict(detail));
    }
    Ok(task_ids
        .first()
        .map(|task_id| parse_domain_id::<TaskId>(task_id))
        .transpose()?)
}

fn insert_email_import_task(
    transaction: &rusqlite::Transaction<'_>,
    messages: &[EmailMessageSnapshot<'_>],
    draft: &EmailTaskDraft<'_>,
    title: &str,
    description: &str,
) -> Result<TaskId, TaskStoreError> {
    let hive_id: String = transaction.query_row(
        "SELECT hive_id FROM local_hive_identity WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let task_id = TaskId::new();
    let (workspace, session_id) = email_import_worker(transaction, draft.worker_id)?;
    transaction.execute(
        "INSERT INTO tasks (
             id, hive_id, title, description, priority, workspace, state,
             assigned_worker_id, position
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
             COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
        params![
            task_id.to_string(),
            hive_id,
            title,
            description,
            draft.priority.to_string(),
            workspace,
            draft.state.to_string(),
            draft.worker_id.map(|id| id.to_string()),
        ],
    )?;
    insert_email_task_activity(transaction, task_id, messages.len(), draft)?;
    if let (Some(worker_id), Some(session_id)) = (draft.worker_id, session_id) {
        queue_email_task_dispatch(transaction, task_id, worker_id, &session_id)?;
    }
    Ok(task_id)
}

fn email_import_worker(
    transaction: &rusqlite::Transaction<'_>,
    worker_id: Option<WorkerId>,
) -> Result<(String, Option<String>), TaskStoreError> {
    let Some(worker_id) = worker_id else {
        return Ok(("email://inbox".to_string(), None));
    };
    transaction
        .query_row(
            "SELECT profile.workspace, session.session_id
             FROM worker_profiles profile
             LEFT JOIN worker_sessions session
               ON session.worker_id = profile.id AND session.ended_at IS NULL
             WHERE profile.id = ?1 AND profile.role != 'queen'",
            [worker_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?
        .ok_or(TaskStoreError::WorkerNotFound)
}

fn insert_email_task_activity(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    message_count: usize,
    draft: &EmailTaskDraft<'_>,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "INSERT INTO task_activity (task_id, kind, to_state, note, actor_kind)
         VALUES (?1, 'created', 'draft', ?2, 'email')",
        params![
            task_id.to_string(),
            format!(
                "Imported from {message_count} email{}",
                if message_count == 1 { "" } else { "s" }
            )
        ],
    )?;
    if draft.state == TaskState::Ready {
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, from_state, to_state, actor_kind)
             VALUES (?1, 'state_changed', 'draft', 'ready', 'email')",
            [task_id.to_string()],
        )?;
    }
    if draft.worker_id.is_some() {
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind)
             VALUES (?1, 'assigned', 'email')",
            [task_id.to_string()],
        )?;
    }
    Ok(())
}

fn queue_email_task_dispatch(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    worker_id: WorkerId,
    session_id: &str,
) -> Result<(), TaskStoreError> {
    let queued: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
        [],
        |row| row.get(0),
    )?;
    if queued >= 256 {
        return Err(TaskStoreError::TaskDispatchQueueFull);
    }
    let assignment_id = Uuid::now_v7().to_string();
    transaction.execute(
        "INSERT INTO task_assignments (id, task_id, worker_session_id) VALUES (?1, ?2, ?3)",
        params![assignment_id, task_id.to_string(), session_id],
    )?;
    transaction.execute(
        "INSERT INTO task_dispatches (assignment_id, task_id, worker_id, state)
         VALUES (?1, ?2, ?3, 'queued')",
        params![assignment_id, task_id.to_string(), worker_id.to_string()],
    )?;
    Ok(())
}

fn insert_email_sources(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    messages: &[EmailMessageSnapshot<'_>],
) -> Result<(), TaskStoreError> {
    for message in messages {
        let source_id = Uuid::now_v7().to_string();
        transaction.execute(
            // ON CONFLICT because the row may still exist from a task that has
            // since been REMOVED. The check above deliberately ignores those,
            // so without this the import walks into the UNIQUE constraint and
            // surfaces as an opaque 503 -- trading one confusing failure for
            // another. Reassigning keeps the message addressable and moves it
            // to the task that now owns it.
            "INSERT INTO email_message_links (
                 id, task_id, integration_id, message_id, conversation_id,
                 internet_message_id, sender_name, sender_address, received_at, web_url
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (integration_id, message_id) DO UPDATE SET
                 task_id = excluded.task_id,
                 conversation_id = excluded.conversation_id,
                 internet_message_id = excluded.internet_message_id,
                 sender_name = excluded.sender_name,
                 sender_address = excluded.sender_address,
                 received_at = excluded.received_at,
                 web_url = excluded.web_url",
            params![
                source_id,
                task_id.to_string(),
                message.integration_id.trim(),
                message.message_id.trim(),
                message.conversation_id.trim(),
                message.internet_message_id.map(str::trim),
                message.sender_name.trim(),
                message.sender_address.trim(),
                message.received_at,
                message.web_url.trim(),
            ],
        )?;
        insert_email_attachments(transaction, task_id, &source_id, message.attachments)?;
    }
    Ok(())
}

fn insert_email_attachments(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    source_id: &str,
    attachments: &[EmailAttachmentSnapshot<'_>],
) -> Result<(), TaskStoreError> {
    for attachment in attachments {
        transaction.execute(
            // ON CONFLICT because one email can legitimately carry the SAME
            // FILE TWICE. storage_name is a content hash, so an image that is
            // both inline in the body and attached, or simply attached twice,
            // produces two rows with identical (source_id, storage_name) and
            // hits the UNIQUE constraint.
            //
            // That aborted the whole import with a SQL error, which the API
            // reports as "task persistence is temporarily unavailable" — so a
            // duplicate screenshot in one message looked like the database was
            // down, and no amount of editing the task could clear it. Measured
            // 2026-08-29 on the operator's Inbox:
            //   UNIQUE constraint failed: email_task_attachments.source_id,
            //   email_task_attachments.storage_name
            //
            // Keeping the first row is right: both rows name the same stored
            // bytes, so nothing is lost and the attachment still resolves.
            "INSERT INTO email_task_attachments (
                 id, source_id, task_id, storage_name, display_name, media_type,
                 byte_size, is_inline, content_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (source_id, storage_name) DO NOTHING",
            params![
                Uuid::now_v7().to_string(),
                source_id,
                task_id.to_string(),
                attachment.storage_name.trim(),
                attachment.display_name.trim(),
                attachment.media_type.trim(),
                i64::try_from(attachment.byte_size)
                    .map_err(|_| TaskStoreError::InvalidEmailAttachment)?,
                attachment.inline,
                attachment.content_id.map(str::trim),
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn migrate_email_intake(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS email_message_links (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             integration_id TEXT NOT NULL,
             message_id TEXT NOT NULL,
             conversation_id TEXT NOT NULL,
             internet_message_id TEXT,
             sender_name TEXT NOT NULL,
             sender_address TEXT NOT NULL,
             received_at INTEGER NOT NULL,
             web_url TEXT NOT NULL,
             imported_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE (integration_id, message_id)
         );
         CREATE INDEX IF NOT EXISTS email_messages_by_conversation
             ON email_message_links(integration_id, conversation_id, received_at DESC);
         CREATE INDEX IF NOT EXISTS email_messages_by_task
             ON email_message_links(task_id, received_at, imported_at, id);
         CREATE TABLE IF NOT EXISTS email_task_attachments (
             id TEXT PRIMARY KEY,
             source_id TEXT NOT NULL REFERENCES email_message_links(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             storage_name TEXT NOT NULL,
             display_name TEXT NOT NULL,
             media_type TEXT NOT NULL,
             byte_size INTEGER NOT NULL CHECK (byte_size > 0),
             is_inline INTEGER NOT NULL CHECK (is_inline IN (0,1)),
             content_id TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE (source_id, storage_name)
         );
         CREATE INDEX IF NOT EXISTS email_attachments_by_task
             ON email_task_attachments(task_id, created_at, id);
         CREATE TABLE IF NOT EXISTS task_deployments (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             environment TEXT NOT NULL,
             reference TEXT NOT NULL,
             deployed_at INTEGER NOT NULL,
             approved_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE (task_id, environment, reference)
         );
         CREATE INDEX IF NOT EXISTS task_deployments_by_task
             ON task_deployments(task_id, deployed_at DESC);
         CREATE TABLE IF NOT EXISTS email_reply_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
             body TEXT NOT NULL,
             state TEXT NOT NULL CHECK (
                 state IN ('draft','queued','dispatching','delivered','uncertain','cancelled')
             ),
             idempotency_key TEXT NOT NULL UNIQUE,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             provider_reply_id TEXT,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL AND provider_reply_id IS NOT NULL)
                 OR state <> 'delivered')
         );
         CREATE INDEX IF NOT EXISTS email_reply_delivery_queue
             ON email_reply_deliveries(state, available_at, created_at);
         CREATE TRIGGER IF NOT EXISTS email_reply_requires_completed_deployment
             BEFORE INSERT ON email_reply_deliveries
             WHEN NOT EXISTS (
                 SELECT 1 FROM tasks task
                 JOIN task_deployments deployment ON deployment.task_id = task.id
                 WHERE task.id = NEW.task_id AND task.state = 'completed'
             )
             BEGIN SELECT RAISE(ABORT, 'Email replies require completed deployed work'); END;
         PRAGMA user_version = 41;",
    )
}

pub(crate) fn migrate_email_multi_source(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
         ALTER TABLE email_reply_deliveries RENAME TO email_reply_deliveries_v40;
         ALTER TABLE email_task_attachments RENAME TO email_task_attachments_v40;
         ALTER TABLE email_message_links RENAME TO email_message_links_v40;
         DROP INDEX IF EXISTS email_reply_delivery_queue;
         DROP INDEX IF EXISTS email_messages_by_conversation;
         DROP INDEX IF EXISTS email_messages_by_task;
         DROP INDEX IF EXISTS email_attachments_by_task;

         CREATE TABLE email_message_links (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             integration_id TEXT NOT NULL,
             message_id TEXT NOT NULL,
             conversation_id TEXT NOT NULL,
             internet_message_id TEXT,
             sender_name TEXT NOT NULL,
             sender_address TEXT NOT NULL,
             received_at INTEGER NOT NULL,
             web_url TEXT NOT NULL,
             imported_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE (integration_id, message_id)
         );
         INSERT INTO email_message_links (
             id, task_id, integration_id, message_id, conversation_id,
             internet_message_id, sender_name, sender_address, received_at,
             web_url, imported_at
         ) SELECT task_id, task_id, integration_id, message_id, conversation_id,
                  internet_message_id, sender_name, sender_address, received_at,
                  web_url, imported_at
             FROM email_message_links_v40;
         CREATE INDEX email_messages_by_task
             ON email_message_links(task_id, received_at, imported_at, id);
         CREATE INDEX email_messages_by_conversation
             ON email_message_links(integration_id, conversation_id, received_at DESC);

         CREATE TABLE email_task_attachments (
             id TEXT PRIMARY KEY,
             source_id TEXT NOT NULL REFERENCES email_message_links(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             storage_name TEXT NOT NULL,
             display_name TEXT NOT NULL,
             media_type TEXT NOT NULL,
             byte_size INTEGER NOT NULL CHECK (byte_size > 0),
             is_inline INTEGER NOT NULL CHECK (is_inline IN (0,1)),
             content_id TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE (source_id, storage_name)
         );
         INSERT INTO email_task_attachments (
             id, source_id, task_id, storage_name, display_name, media_type,
             byte_size, is_inline, content_id, created_at
         ) SELECT id, task_id, task_id, storage_name, display_name, media_type,
                  byte_size, is_inline, content_id, created_at
             FROM email_task_attachments_v40;
         CREATE INDEX email_attachments_by_task
             ON email_task_attachments(task_id, created_at, id);

         CREATE TABLE email_reply_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
             body TEXT NOT NULL,
             state TEXT NOT NULL CHECK (
                 state IN ('draft','queued','dispatching','delivered','uncertain','cancelled')
             ),
             idempotency_key TEXT NOT NULL UNIQUE,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             provider_reply_id TEXT,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL AND provider_reply_id IS NOT NULL)
                 OR state <> 'delivered')
         );
         INSERT INTO email_reply_deliveries (
             id, task_id, body, state, idempotency_key, attempts, available_at,
             attempted_at, delivered_at, provider_reply_id, last_error, created_at, updated_at
         ) SELECT id, task_id, body, state, idempotency_key, attempts, available_at,
                  attempted_at, delivered_at, provider_reply_id, last_error, created_at, updated_at
             FROM email_reply_deliveries_v40;
         CREATE INDEX email_reply_delivery_queue
             ON email_reply_deliveries(state, available_at, created_at);
         CREATE TRIGGER email_reply_requires_completed_deployment
             BEFORE INSERT ON email_reply_deliveries
             WHEN NOT EXISTS (
                 SELECT 1 FROM tasks task
                 JOIN task_deployments deployment ON deployment.task_id = task.id
                 WHERE task.id = NEW.task_id AND task.state = 'completed'
             )
             BEGIN SELECT RAISE(ABORT, 'Email replies require completed deployed work'); END;

         DROP TABLE email_reply_deliveries_v40;
         DROP TABLE email_task_attachments_v40;
         DROP TABLE email_message_links_v40;
         PRAGMA user_version = 41;",
    )
}

pub(crate) fn migrate_email_reply_targets(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS email_reply_targets (
             id TEXT PRIMARY KEY,
             reply_id TEXT NOT NULL REFERENCES email_reply_deliveries(id) ON DELETE CASCADE,
             source_id TEXT NOT NULL REFERENCES email_message_links(id) ON DELETE RESTRICT,
             state TEXT NOT NULL CHECK (
                 state IN ('draft','queued','dispatching','delivered','uncertain','cancelled')
             ),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             provider_reply_id TEXT,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE (reply_id, source_id),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL AND provider_reply_id IS NOT NULL)
                 OR state <> 'delivered')
         );
         CREATE INDEX IF NOT EXISTS email_reply_target_queue
             ON email_reply_targets(state, available_at, created_at);
         INSERT OR IGNORE INTO email_reply_targets (
             id, reply_id, source_id, state, attempts, available_at, attempted_at,
             delivered_at, provider_reply_id, last_error, created_at, updated_at
         )
         SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-7' ||
                    substr(lower(hex(randomblob(2))), 2) || '-' ||
                    substr('89ab', abs(random()) % 4 + 1, 1) ||
                    substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6))),
                reply.id,
                (SELECT source.id FROM email_message_links source
                  WHERE source.task_id = reply.task_id
                  ORDER BY source.received_at, source.imported_at, source.id LIMIT 1),
                reply.state, reply.attempts, reply.available_at, reply.attempted_at,
                reply.delivered_at, reply.provider_reply_id, reply.last_error,
                reply.created_at, reply.updated_at
           FROM email_reply_deliveries reply
          WHERE EXISTS (SELECT 1 FROM email_message_links source WHERE source.task_id = reply.task_id);
         INSERT OR IGNORE INTO email_reply_targets (
             id, reply_id, source_id, state, attempts, available_at, created_at, updated_at
         )
         SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-7' ||
                    substr(lower(hex(randomblob(2))), 2) || '-' ||
                    substr('89ab', abs(random()) % 4 + 1, 1) ||
                    substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6))),
                reply.id, source.id, 'draft', 0, reply.available_at,
                reply.created_at, reply.updated_at
           FROM email_reply_deliveries reply
           JOIN email_message_links source ON source.task_id = reply.task_id
          WHERE reply.state = 'draft';
         PRAGMA user_version = 43;",
    )
}

fn validate_email_message(message: &EmailMessageSnapshot<'_>) -> Result<(), TaskStoreError> {
    let body = message.body_text.trim();
    // NOT validate_text: that raises InvalidTitle, whose message talks about a
    // task title. The operator's own title governs this import and is checked
    // separately; the subject only has to be storable.
    if !bounded_text(&email_task_title(message), MAX_EMAIL_SUBJECT_BYTES) {
        return Err(TaskStoreError::InvalidEmailMessage);
    }
    validate_description(body)?;
    if !bounded_text(message.integration_id.trim(), MAX_SOURCE_ID_BYTES)
        || !bounded_text(message.message_id.trim(), MAX_SOURCE_ID_BYTES)
        || !bounded_text(message.conversation_id.trim(), MAX_CONVERSATION_ID_BYTES)
        || message
            .internet_message_id
            .is_some_and(|value| !bounded_text(value.trim(), MAX_INTERNET_MESSAGE_ID_BYTES))
        || message.sender_name.trim().len() > MAX_SENDER_NAME_BYTES
        || !bounded_text(message.sender_address.trim(), MAX_SENDER_ADDRESS_BYTES)
        || !message.sender_address.contains('@')
        || message.received_at <= 0
        || !bounded_text(message.web_url.trim(), MAX_WEB_URL_BYTES)
        || !message.web_url.trim().starts_with("https://")
        || body.contains('\0')
        || message.attachments.len() > MAX_EMAIL_ATTACHMENTS
    {
        return Err(TaskStoreError::InvalidEmailMessage);
    }
    let mut total = 0_u64;
    for attachment in message.attachments {
        if !bounded_text(attachment.storage_name.trim(), MAX_ATTACHMENT_NAME_BYTES)
            || !attachment
                .storage_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            || !bounded_text(attachment.display_name.trim(), MAX_ATTACHMENT_NAME_BYTES)
            || !bounded_text(attachment.media_type.trim(), MAX_MEDIA_TYPE_BYTES)
            || attachment.byte_size == 0
            || attachment.byte_size > MAX_EMAIL_ATTACHMENT_BYTES
            || attachment
                .content_id
                .is_some_and(|value| !bounded_text(value.trim(), MAX_CONTENT_ID_BYTES))
        {
            return Err(TaskStoreError::InvalidEmailAttachment);
        }
        total = total
            .checked_add(attachment.byte_size)
            .ok_or(TaskStoreError::InvalidEmailAttachment)?;
    }
    if total > MAX_EMAIL_ATTACHMENT_TOTAL_BYTES {
        return Err(TaskStoreError::InvalidEmailAttachment);
    }
    Ok(())
}

/// A task title derived from a message, for the paths that have no operator
/// title to use.
///
/// CLAMPED rather than refused. A subject longer than a task title is a normal
/// thing for mail to contain, and refusing the import over it strands the
/// message with an error about a field the operator did not write. Truncation
/// is on a char boundary so it can never split a multi-byte character.
fn email_task_title(message: &EmailMessageSnapshot<'_>) -> String {
    clamp_to_title_bytes(&email_subject_or_sender(message))
}

fn clamp_to_title_bytes(value: &str) -> String {
    if value.len() <= crate::MAX_TASK_TITLE_BYTES {
        return value.to_owned();
    }
    let mut end = crate::MAX_TASK_TITLE_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim_end().to_owned()
}

fn email_subject_or_sender(message: &EmailMessageSnapshot<'_>) -> String {
    let subject = message.subject.trim();
    if subject.is_empty() {
        let sender = message.sender_name.trim();
        if sender.is_empty() {
            "Email issue".to_owned()
        } else {
            format!("Email from {sender}")
        }
    } else {
        subject.to_owned()
    }
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.contains('\0')
}

fn email_task_link_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailTaskLink> {
    Ok(EmailTaskLink {
        id: row.get(0)?,
        task_id: parse_domain_id::<TaskId>(&row.get::<_, String>(1)?)?,
        integration_id: row.get(2)?,
        message_id: row.get(3)?,
        conversation_id: row.get(4)?,
        internet_message_id: row.get(5)?,
        sender_name: row.get(6)?,
        sender_address: row.get(7)?,
        received_at: row.get(8)?,
        web_url: row.get(9)?,
        imported_at: row.get(10)?,
        attachments: Vec::new(),
    })
}

fn email_attachment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailTaskAttachment> {
    Ok(EmailTaskAttachment {
        storage_name: row.get(0)?,
        display_name: row.get(1)?,
        media_type: row.get(2)?,
        byte_size: row.get(3)?,
        inline: row.get(4)?,
        content_id: row.get(5)?,
    })
}

fn deployment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskDeploymentRecord> {
    Ok(TaskDeploymentRecord {
        id: row.get(0)?,
        task_id: parse_domain_id::<TaskId>(&row.get::<_, String>(1)?)?,
        environment: row.get(2)?,
        reference: row.get(3)?,
        deployed_at: row.get(4)?,
        recorded_at: row.get(5)?,
    })
}

fn email_reply_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailReplyDispatch> {
    Ok(EmailReplyDispatch {
        id: row.get(0)?,
        task_id: parse_domain_id::<TaskId>(&row.get::<_, String>(1)?)?,
        body: row.get(2)?,
        state: row
            .get::<_, String>(3)?
            .parse()
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        idempotency_key: row.get(4)?,
        attempts: row.get(5)?,
        available_at: row.get(6)?,
        attempted_at: row.get(7)?,
        delivered_at: row.get(8)?,
        provider_reply_id: row.get(9)?,
        last_error: row.get(10)?,
        targets: Vec::new(),
    })
}

fn email_reply_by_id(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<EmailReplyDispatch>, TaskStoreError> {
    let mut reply = connection
        .query_row(
            "SELECT id, task_id, body, state, idempotency_key, attempts,
                    available_at, attempted_at, delivered_at, provider_reply_id, last_error
             FROM email_reply_deliveries WHERE id = ?1",
            [id],
            email_reply_from_row,
        )
        .optional()?;
    if let Some(reply) = &mut reply {
        reply.targets = email_reply_targets(connection, &reply.id)?;
        if !reply.targets.is_empty() {
            reply.state = aggregate_reply_state(&reply.targets);
            reply.attempts = reply
                .targets
                .iter()
                .map(|target| target.attempts)
                .max()
                .unwrap_or(0);
            reply.attempted_at = reply
                .targets
                .iter()
                .filter_map(|target| target.attempted_at)
                .max();
            reply.delivered_at = reply
                .targets
                .iter()
                .filter_map(|target| target.delivered_at)
                .max();
            reply.last_error = reply
                .targets
                .iter()
                .find_map(|target| target.last_error.clone());
        }
    }
    Ok(reply)
}

fn email_reply_targets(
    connection: &rusqlite::Connection,
    reply_id: &str,
) -> Result<Vec<EmailReplyTarget>, TaskStoreError> {
    let mut statement = connection.prepare(
        "SELECT target.id, source.id, source.sender_name, source.sender_address,
                source.web_url, target.state, target.attempts, target.attempted_at,
                target.delivered_at, target.last_error
           FROM email_reply_targets target
           JOIN email_message_links source ON source.id = target.source_id
          WHERE target.reply_id = ?1
          ORDER BY source.received_at, source.imported_at, source.id",
    )?;
    Ok(statement
        .query_map([reply_id], |row| {
            Ok(EmailReplyTarget {
                id: row.get(0)?,
                source_id: row.get(1)?,
                sender_name: row.get(2)?,
                sender_address: row.get(3)?,
                web_url: row.get(4)?,
                state: row
                    .get::<_, String>(5)?
                    .parse()
                    .map_err(|()| rusqlite::Error::InvalidQuery)?,
                attempts: row.get(6)?,
                attempted_at: row.get(7)?,
                delivered_at: row.get(8)?,
                last_error: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn aggregate_reply_state(targets: &[EmailReplyTarget]) -> EmailReplyState {
    if targets
        .iter()
        .all(|target| target.state == EmailReplyState::Delivered)
    {
        EmailReplyState::Delivered
    } else if targets
        .iter()
        .any(|target| target.state == EmailReplyState::Uncertain)
    {
        EmailReplyState::Uncertain
    } else if targets
        .iter()
        .any(|target| target.state == EmailReplyState::Dispatching)
    {
        EmailReplyState::Dispatching
    } else if targets
        .iter()
        .any(|target| target.state == EmailReplyState::Queued)
    {
        EmailReplyState::Queued
    } else if targets
        .iter()
        .any(|target| target.state == EmailReplyState::Cancelled)
    {
        EmailReplyState::Cancelled
    } else {
        EmailReplyState::Draft
    }
}

fn reply_for_target(
    connection: &rusqlite::Connection,
    target_id: &str,
) -> Result<EmailReplyDispatch, TaskStoreError> {
    let reply_id = connection
        .query_row(
            "SELECT reply_id FROM email_reply_targets WHERE id = ?1",
            [target_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(TaskStoreError::InvalidEmailReply)?;
    refresh_email_reply_summary(connection, &reply_id)
}

fn refresh_email_reply_summary(
    connection: &rusqlite::Connection,
    reply_id: &str,
) -> Result<EmailReplyDispatch, TaskStoreError> {
    let reply =
        email_reply_by_id(connection, reply_id)?.ok_or(TaskStoreError::InvalidEmailReply)?;
    let provider_reply_id =
        (reply.state == EmailReplyState::Delivered).then(|| format!("fanout:{reply_id}"));
    connection.execute(
        "UPDATE email_reply_deliveries
            SET state = ?2, attempts = ?3, attempted_at = ?4, delivered_at = ?5,
                provider_reply_id = ?6, last_error = ?7, updated_at = unixepoch()
          WHERE id = ?1",
        params![
            reply_id,
            reply.state.to_string(),
            reply.attempts,
            reply.attempted_at,
            reply.delivered_at,
            provider_reply_id,
            reply.last_error,
        ],
    )?;
    email_reply_by_id(connection, reply_id)?.ok_or(TaskStoreError::InvalidEmailReply)
}

fn resolve_email_reply_target_id(
    connection: &rusqlite::Connection,
    id: &str,
    state: &str,
) -> Result<String, TaskStoreError> {
    connection
        .query_row(
            "SELECT id FROM email_reply_targets
              WHERE state = ?2 AND (id = ?1 OR reply_id = ?1)
              ORDER BY CASE WHEN id = ?1 THEN 0 ELSE 1 END, created_at, id LIMIT 1",
            params![id, state],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(TaskStoreError::InvalidEmailReply)
}

/// Lets the worker that did the work answer the person who wrote in.
///
/// The trigger required a task to be `completed` before any reply could be
/// inserted. A worker can only report Active, Blocked or Review for its own
/// assignment, so the one actor that knows where the work is running and what
/// to say could never satisfy it — and the reply fell to whoever closed the
/// task later, which is how the person who wrote in ended up waiting while the
/// operator drafted it by hand.
///
/// Review is included, and nothing else is loosened: a deployment record is
/// still required, and this still only drafts. The operator sends.
///
/// # Errors
/// Returns an error when the step cannot be applied.
pub(super) fn migrate_email_reply_from_review(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let replies_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'email_reply_deliveries')",
        [],
        |row| row.get(0),
    )?;
    if replies_exist {
        transaction.execute_batch(
            "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
             CREATE TRIGGER email_reply_requires_completed_deployment
                 BEFORE INSERT ON email_reply_deliveries
                 WHEN NOT EXISTS (
                     SELECT 1 FROM tasks task
                     JOIN task_deployments deployment ON deployment.task_id = task.id
                     WHERE task.id = NEW.task_id AND task.state IN ('completed', 'review')
                 )
                 BEGIN SELECT RAISE(ABORT, 'Email replies require deployed work in review or completed'); END;",
        )?;
    }
    transaction.pragma_update(None, "user_version", EMAIL_REPLY_FROM_REVIEW_SCHEMA_VERSION)
}

/// A reply may follow work that legitimately shipped nothing.
///
/// The trigger demanded a row in `task_deployments`. An APPROVED no-deployment
/// exemption did not satisfy it, so a task closed on "nothing was built" could
/// never have a reply created at all — the person who wrote in was structurally
/// unanswerable, and the database would have rejected the answer.
///
/// Found on a real task: a worker investigated a question, established that no
/// change was needed, wrote "Answer drafted for Ryan", and Queen approved the
/// exemption. Completing that way is correct and is what the exemption exists
/// for. Being unable to then tell Ryan is not.
///
/// The operator's own ruling makes this worse than an inconvenience: they said
/// the only thing they verify on an email task is the draft, and the worker
/// verifies whether anything is running. The old rule sent them to record a
/// deployment that did not exist and should not exist.
///
/// Approved only. A CLAIMED exemption nobody has approved still blocks a reply,
/// which matches what closes a task: claiming is not evidence.
pub(super) fn migrate_reply_allows_approved_exemption(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
         CREATE TRIGGER email_reply_requires_completed_deployment
             BEFORE INSERT ON email_reply_deliveries
             WHEN NOT EXISTS (
                 SELECT 1 FROM tasks task
                 WHERE task.id = NEW.task_id
                   AND task.state IN ('completed', 'review')
                   AND (
                       EXISTS (SELECT 1 FROM task_deployments deployment
                                WHERE deployment.task_id = task.id)
                       OR EXISTS (SELECT 1 FROM task_completion_exemptions exemption
                                   WHERE exemption.task_id = task.id
                                     AND exemption.approved_at IS NOT NULL)
                   )
             )
             BEGIN SELECT RAISE(ABORT, 'Email replies require deployed work, or an approved no-deployment exemption, in review or completed'); END;
         PRAGMA user_version = 93;",
    )
}

/// Moves the evidence rule from the DRAFT to the SEND.
///
/// The trigger has always fired BEFORE INSERT on `email_reply_deliveries` —
/// that is, on writing words into a textarea. Sending tested `state = 'draft'`
/// and nothing else. So the database refused to let a worker WRITE an unevidenced
/// reply, and would happily have SENT one.
///
/// It also made the reply unwritable in practice. Evidence is approved by Queen
/// after the worker's turn ends, so the worker's only window to draft closed
/// before the thing it was waiting on could exist. The operator was handed an
/// empty box on a completed task and asked to write the reply themselves.
///
/// So: two triggers instead of one. Drafting asks only that the work is done
/// being worked. Sending asks for everything the old rule asked for, at the
/// moment it actually matters. Nothing reaches a person any earlier than before.
pub(super) fn migrate_reply_evidence_guards_the_send(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        // All three drops, because this chain is re-run over databases that
        // already carry the new triggers — every schema-vN migration test builds
        // a modern store, tears it back to an old version and migrates forward
        // again. A CREATE that is not preceded by its own DROP fails there with
        // "already exists", and fourteen of those tests said so at once.
        "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
         DROP TRIGGER IF EXISTS email_reply_draft_requires_finished_work;
         DROP TRIGGER IF EXISTS email_reply_send_requires_evidence;
         CREATE TRIGGER email_reply_draft_requires_finished_work
             BEFORE INSERT ON email_reply_deliveries
             WHEN NOT EXISTS (
                 SELECT 1 FROM tasks task
                 WHERE task.id = NEW.task_id
                   AND task.state IN ('completed', 'review')
             )
             BEGIN SELECT RAISE(ABORT, 'An email reply can be drafted once the task is in review or completed'); END;
         CREATE TRIGGER email_reply_send_requires_evidence
             BEFORE UPDATE OF state ON email_reply_deliveries
             WHEN NEW.state = 'queued' AND OLD.state <> 'queued'
              AND NOT EXISTS (
                 SELECT 1 FROM tasks task
                 WHERE task.id = NEW.task_id
                   AND task.state IN ('completed', 'review')
                   AND (
                       EXISTS (SELECT 1 FROM task_deployments deployment
                                WHERE deployment.task_id = task.id)
                       OR EXISTS (SELECT 1 FROM task_completion_exemptions exemption
                                   WHERE exemption.task_id = task.id
                                     AND exemption.approved_at IS NOT NULL)
                   )
             )
             BEGIN SELECT RAISE(ABORT, 'An email reply cannot be sent without a recorded deployment or an approved no-deployment exemption'); END;
         PRAGMA user_version = 108;",
    )
}

/// One waiting requester, built from the row the queue query returned.
///
/// Extracted so that query stays under the line limit. An eleven-column
/// destructure and a struct literal is most of a function on its own, and none
/// of it is what the query is about.
type UnansweredEmailRow = (
    String,
    String,
    String,
    String,
    i64,
    bool,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
);

fn unanswered_email_task(row: UnansweredEmailRow) -> Result<UnansweredEmailTask, TaskStoreError> {
    let (
        id,
        title,
        sender_name,
        sender_address,
        received_at,
        drafted,
        sending,
        draft_id,
        draft_body,
        worker_name,
        thread_count,
        delivery_failure,
        no_reply_reason,
    ) = row;
    Ok(UnansweredEmailTask {
        task_id: TaskId::from_str(&id)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
        title,
        sender_name,
        sender_address,
        received_at,
        drafted,
        sending,
        draft_id,
        draft_body,
        worker_name,
        thread_count: usize::try_from(thread_count).unwrap_or(1).max(1),
        delivery_failure,
        no_reply_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message<'a>(attachments: &'a [EmailAttachmentSnapshot<'a>]) -> EmailMessageSnapshot<'a> {
        EmailMessageSnapshot {
            integration_id: "operator-outlook",
            message_id: "AAMk-message-1",
            conversation_id: "AAQk-conversation-1",
            internet_message_id: Some("<issue-1@example.test>"),
            subject: "The member form is not saving",
            sender_name: "A Member",
            sender_address: "member@example.test",
            received_at: 1_786_730_000,
            web_url: "https://outlook.office.com/mail/inbox/id/AAMk-message-1",
            body_text: "The form loses my phone number after I press Save.",
            attachments,
        }
    }

    /// One email carrying the same file twice must still import.
    ///
    /// `storage_name` is a content hash, so an image that is both inline in the
    /// body and attached — or simply attached twice — produces two rows with
    /// the same (`source_id`, `storage_name`). That hit the UNIQUE constraint and
    /// aborted the import with a SQL error, which the API reports as "task
    /// persistence is temporarily unavailable". The operator saw a database
    /// outage; the cause was a duplicated screenshot in one message.
    /// Awaiting-release work settles itself the moment it ships.
    ///
    /// The state exists because "this ships nothing, ever" and "this ships
    /// later" were being expressed by the same exemption, and the second closed
    /// work on a claim that was false. Parking work is only worth doing if
    /// nothing then has to remember to come back for it, so this is the
    /// assertion the whole state stands on.
    #[test]
    fn awaiting_release_work_completes_itself_when_the_deployment_lands() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Ship the thing", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        store
            .transition_task(task.id, TaskState::AwaitingRelease)
            .unwrap();

        store
            .record_task_deployment(task.id, "production", "abc123", 2_000)
            .unwrap();

        assert_eq!(
            store.get_task(task.id).unwrap().state,
            TaskState::Completed,
            "a recorded deployment is the evidence the completion gate asks for, \
             so nobody should have to click to agree with it"
        );
    }

    /// THE NEGATIVE: shipping something is not somebody agreeing it is done.
    ///
    /// Work still in Review has not been accepted. A deployment recorded there
    /// is evidence, and evidence is not approval — closing on it would delete
    /// the review rather than satisfy it.
    #[test]
    fn a_deployment_against_work_still_in_review_does_not_close_it() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Ship the thing", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();

        store
            .record_task_deployment(task.id, "production", "abc123", 2_000)
            .unwrap();

        assert_eq!(
            store.get_task(task.id).unwrap().state,
            TaskState::Review,
            "review is still owed, and a deployment does not discharge it"
        );
    }

    #[test]
    fn one_email_carrying_the_same_file_twice_still_imports() {
        let store = TaskStore::in_memory().unwrap();
        // Identical bytes stored once, referenced twice: inline and attached.
        let duplicated = [
            EmailAttachmentSnapshot {
                storage_name: "sha256-screen.png",
                display_name: "screen.png",
                media_type: "image/png",
                byte_size: 1_024,
                inline: true,
                content_id: Some("screen-1"),
            },
            EmailAttachmentSnapshot {
                storage_name: "sha256-screen.png",
                display_name: "screen.png",
                media_type: "image/png",
                byte_size: 1_024,
                inline: false,
                content_id: None,
            },
        ];

        let imported = store
            .import_email_messages(
                std::slice::from_ref(&message(&duplicated)),
                &EmailTaskDraft {
                    title: "A report with the screenshot twice",
                    description: "x",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .expect("a duplicated attachment must not abort the whole import");

        assert!(imported.created);
        assert_eq!(
            imported.source.attachments.len(),
            1,
            "the duplicate collapses to one row naming the same stored bytes"
        );
    }

    /// Removing a task frees the emails it held.
    ///
    /// Found by causing it: a message imported to verify a fix stayed attached
    /// to that task forever. Removal is a soft delete, `existing_email_task_id`
    /// queried the links alone, and no route in the product could break the
    /// bond — so a single mistaken import poisoned a message permanently and
    /// the only remedy was a direct database write.
    #[test]
    fn removing_a_task_frees_the_emails_it_held() {
        let store = TaskStore::in_memory().unwrap();
        let snapshot = message(&[]);
        let first = store
            .import_email_messages(
                std::slice::from_ref(&snapshot),
                &EmailTaskDraft {
                    title: "Imported by mistake",
                    description: "x",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .unwrap();
        assert!(first.created);

        // The operator removes the task the mistake made.
        store
            .remove_task_as(
                first.task.id,
                &swarm_domain::TaskActivityActor::operator(),
                "wrong import",
            )
            .unwrap();

        let second = store
            .import_email_messages(
                std::slice::from_ref(&snapshot),
                &EmailTaskDraft {
                    title: "Where it actually belongs",
                    description: "x",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .expect("the message is free once the task holding it is gone");

        assert!(second.created, "a NEW task, not the removed one revived");
        assert_ne!(second.task.id, first.task.id);
        assert_eq!(second.task.title, "Where it actually belongs");
    }

    /// The refusal now says which messages, and on what.
    ///
    /// It used to say only that the selection "belong to different existing
    /// tasks" — accurate, and leaving the operator to deselect six messages one
    /// at a time to find the one that mattered.
    #[test]
    fn a_mixed_selection_names_the_messages_that_are_already_attached() {
        let store = TaskStore::in_memory().unwrap();
        let held = message(&[]);
        store
            .import_email_messages(
                std::slice::from_ref(&held),
                &EmailTaskDraft {
                    title: "Holds the first message",
                    description: "x",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .unwrap();

        let mut free = message(&[]);
        free.message_id = "AAMk-message-2";
        free.internet_message_id = Some("<issue-2@example.test>");
        free.subject = "A second, unattached report";

        let error = store
            .import_email_messages(
                &[held, free],
                &EmailTaskDraft {
                    title: "Both together",
                    description: "x",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .expect_err("a mixed selection is still refused");

        let text = error.to_string();
        assert!(text.contains("1 of the 2"), "says how many: {text}");
        assert!(
            text.contains("The member form is not saving"),
            "names the message that is attached: {text}"
        );
        assert!(
            text.contains("Holds the first message"),
            "and the task holding it: {text}"
        );
        assert!(
            text.contains("remove the task"),
            "and what to do about it: {text}"
        );
    }

    /// The operator's report, reproduced: "the title was fine, and that
    /// shouldn't matter anyway."
    ///
    /// Both halves were right. Their title WAS fine — the import was refused
    /// over the EMAIL'S subject, which `validate_email_message` ran through
    /// `validate_text`, whose error says "task title must contain 1 to 240
    /// bytes". So the message named the one field the operator could see and
    /// edit, and editing it could never help. A previous fix clamped the typed
    /// title and did not touch this, which is why the error survived it.
    ///
    /// A subject longer than a task title is a normal thing for mail to carry.
    /// It is source metadata and is bounded as such.
    #[test]
    fn a_subject_longer_than_a_task_title_still_imports_under_the_operators_own_title() {
        let store = TaskStore::in_memory().unwrap();
        let long_subject = "Re: ".to_owned() + &"escalation ".repeat(40);
        assert!(
            long_subject.len() > crate::MAX_TASK_TITLE_BYTES,
            "the premise: {} bytes",
            long_subject.len()
        );
        let mut snapshot = message(&[]);
        snapshot.subject = &long_subject;

        let imported = store
            .import_email_messages(
                std::slice::from_ref(&snapshot),
                &EmailTaskDraft {
                    // Short, valid, and entirely the operator's own.
                    title: "Chase the member form escalation",
                    description: "Body kept for context.",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .expect("a long subject must not refuse an import the operator titled correctly");

        assert!(imported.created);
        assert_eq!(
            imported.task.title, "Chase the member form escalation",
            "and the operator's title is what the task carries"
        );
    }

    /// A body only the operator would call long must not strand the message.
    ///
    /// The subject path above clamps rather than refuse, precisely so a message
    /// is never lost to its own length. The body path did the opposite: the
    /// fetcher accepted up to its own ceiling, the store refused above a much
    /// smaller one, and every message in between was fetched and then dropped
    /// during a background sync where no operator ever saw the failure.
    ///
    /// Pinned as an invariant rather than a number, so raising either cap alone
    /// fails here instead of silently reopening the gap.
    #[test]
    fn a_body_as_large_as_the_fetcher_accepts_still_imports() {
        let store = TaskStore::in_memory().unwrap();
        // Multi-byte, so a cap applied in the wrong unit shows up as a failure.
        let long_body = "é".repeat(crate::MAX_TASK_DESCRIPTION_BYTES / 2);
        assert!(
            long_body.len() > 10_000,
            "the body must exceed the cap this test exists to retire"
        );
        let mut snapshot = message(&[]);
        snapshot.body_text = &long_body;

        let imported = store
            .import_email_message(&snapshot, TaskPriority::Normal)
            .expect("a long email imports rather than being dropped mid-sync");

        assert_eq!(
            imported.task.description, long_body,
            "and arrives whole, not truncated to fit"
        );
    }

    /// The other path, where the subject genuinely BECOMES the title.
    ///
    /// Nothing supplies a title there, so refusing would strand the message.
    /// It is clamped instead, on a char boundary.
    #[test]
    fn a_single_message_with_an_overlong_subject_is_clamped_rather_than_refused() {
        let store = TaskStore::in_memory().unwrap();
        // Multi-byte on purpose: a naive byte cut would split one of these.
        let long_subject = "é".repeat(300);
        let mut snapshot = message(&[]);
        snapshot.subject = &long_subject;

        let imported = store
            .import_email_message(&snapshot, TaskPriority::Normal)
            .expect("a long subject is clamped, not refused");

        assert!(
            imported.task.title.len() <= crate::MAX_TASK_TITLE_BYTES,
            "clamped to the limit, {} bytes",
            imported.task.title.len()
        );
        assert!(
            imported.task.title.starts_with('é'),
            "and never split mid-character: {:?}",
            imported.task.title
        );
    }

    #[test]
    fn email_import_is_atomic_bounded_and_idempotent() {
        let store = TaskStore::in_memory().unwrap();
        let attachments = [EmailAttachmentSnapshot {
            storage_name: "sha256-screen.png",
            display_name: "screen.png",
            media_type: "image/png",
            byte_size: 1_024,
            inline: true,
            content_id: Some("screen-1"),
        }];
        let first = store
            .import_email_message(&message(&attachments), TaskPriority::High)
            .unwrap();
        let second = store
            .import_email_message(&message(&attachments), TaskPriority::Low)
            .unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.task.id, second.task.id);
        assert_eq!(first.task.priority, TaskPriority::High);
        assert_eq!(first.task.workspace, "email://inbox");
        assert_eq!(first.source.attachments.len(), 1);
        assert_eq!(
            store.email_task_links().unwrap(),
            vec![first.source.clone()]
        );
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn a_completed_email_task_nobody_answered_is_reported() {
        // A task imported from email carries a person waiting on it. Finishing
        // the work tells them nothing — the reply is a separate deliberate
        // step — and nothing noticed when it never happened, so a worker could
        // close the task and the requester heard nothing at all.
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();

        // Work still in progress is not owed a reply yet.
        assert!(
            store
                .completed_email_tasks_awaiting_a_reply()
                .unwrap()
                .is_empty()
        );

        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }

        let awaiting = store.completed_email_tasks_awaiting_a_reply().unwrap();
        assert_eq!(awaiting.len(), 1);
        assert_eq!(awaiting[0].task_id, imported.task.id);
        assert_eq!(awaiting[0].sender_address, "member@example.test");
        assert!(!awaiting[0].drafted, "nothing has been written yet");
    }

    #[test]
    fn a_written_reply_is_not_a_sent_one() {
        // Drafting is not answering. The requester has still heard nothing
        // until a reply is actually delivered, so a draft left unsent must keep
        // reporting rather than quietly clearing the flag.
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .record_task_deployment(imported.task.id, "production", "release-9", 1_786_730_200)
            .unwrap();
        let draft = store
            .prepare_email_reply(imported.task.id, "Thank you, this is fixed.")
            .unwrap();

        let drafted = store.completed_email_tasks_awaiting_a_reply().unwrap();
        assert_eq!(drafted.len(), 1);
        assert!(drafted[0].drafted, "a draft exists and is worth saying so");

        store.queue_email_reply(&draft.id).unwrap();
        let target = store.claim_email_reply().unwrap().unwrap();
        store
            .complete_email_reply(&target.target_id, "provider-reply-1")
            .unwrap();

        assert!(
            store
                .completed_email_tasks_awaiting_a_reply()
                .unwrap()
                .is_empty(),
            "a delivered reply is what answers the thread"
        );
    }

    #[test]
    fn related_emails_merge_into_one_task_without_losing_source_identity() {
        let store = TaskStore::in_memory().unwrap();
        let first = message(&[]);
        let second = EmailMessageSnapshot {
            integration_id: "operator-outlook",
            message_id: "AAMk-message-2",
            conversation_id: "AAQk-conversation-2",
            internet_message_id: Some("<issue-2@example.test>"),
            subject: "More detail about the member form",
            sender_name: "A Member",
            sender_address: "member@example.test",
            received_at: 1_786_730_100,
            web_url: "https://outlook.office.com/mail/inbox/id/AAMk-message-2",
            body_text: "This also happens after changing the country.",
            attachments: &[],
        };
        let draft = EmailTaskDraft {
            title: "Fix the member form reports",
            description: "Two related reports describe the same outcome.",
            priority: TaskPriority::High,
            worker_id: None,
            state: TaskState::Ready,
        };
        let imported = store
            .import_email_messages(&[first, second], &draft)
            .unwrap();

        assert!(imported.created);
        assert_eq!(imported.task.title, draft.title);
        assert_eq!(imported.task.state, TaskState::Ready);
        assert_eq!(imported.sources.len(), 2);
        assert_ne!(imported.sources[0].id, imported.sources[1].id);
        assert_eq!(imported.sources[0].task_id, imported.task.id);
        assert_eq!(imported.sources[1].task_id, imported.task.id);
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        let existing = store
            .import_email_messages(
                &[
                    message(&[]),
                    EmailMessageSnapshot {
                        integration_id: "operator-outlook",
                        message_id: "AAMk-message-2",
                        conversation_id: "AAQk-conversation-2",
                        internet_message_id: Some("<issue-2@example.test>"),
                        subject: "More detail about the member form",
                        sender_name: "A Member",
                        sender_address: "member@example.test",
                        received_at: 1_786_730_100,
                        web_url: "https://outlook.office.com/mail/inbox/id/AAMk-message-2",
                        body_text: "This also happens after changing the country.",
                        attachments: &[],
                    },
                ],
                &draft,
            )
            .unwrap();
        assert!(!existing.created);
        assert_eq!(existing.task.id, imported.task.id);
        assert_eq!(existing.sources.len(), 2);
    }

    /// The queue says how many people one Send actually reaches.
    ///
    /// A task can be linked to several inbound emails and the reply fans out to
    /// every one of them, but the waiting-reply row carried a single sender —
    /// whichever wrote in first. So a send answering seven threads presented
    /// exactly like a send answering one, and the operator approved it from a
    /// line naming one person. This Hive's own history has replies with five
    /// and seven targets.
    #[test]
    fn a_task_answering_several_threads_says_how_many() {
        let store = TaskStore::in_memory().unwrap();
        let second = EmailMessageSnapshot {
            integration_id: "operator-outlook",
            message_id: "AAMk-message-2",
            conversation_id: "AAQk-conversation-2",
            internet_message_id: Some("<issue-2@example.test>"),
            subject: "A second report",
            sender_name: "Another Member",
            sender_address: "another@example.test",
            received_at: 1_786_730_100,
            web_url: "https://outlook.office.com/mail/inbox/id/AAMk-message-2",
            body_text: "The same form fails for me.",
            attachments: &[],
        };
        let imported = store
            .import_email_messages(
                &[message(&[]), second],
                &EmailTaskDraft {
                    title: "Fix both reported form failures",
                    description: "Two people reported the same outcome.",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Ready,
                },
            )
            .unwrap();
        for state in [TaskState::Active, TaskState::Review, TaskState::Completed] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .record_task_deployment(imported.task.id, "production", "release-43", 1_786_730_200)
            .unwrap();

        let awaiting = store.completed_email_tasks_awaiting_a_reply().unwrap();

        assert_eq!(
            awaiting.len(),
            1,
            "one task, however many threads it answers"
        );
        assert_eq!(
            awaiting[0].thread_count, 2,
            "the row named one sender and hid the other, which is the whole defect"
        );
    }

    #[test]
    fn merged_email_reply_tracks_every_thread_independently() {
        let store = TaskStore::in_memory().unwrap();
        let first = message(&[]);
        let second = EmailMessageSnapshot {
            integration_id: "operator-outlook",
            message_id: "AAMk-message-2",
            conversation_id: "AAQk-conversation-2",
            internet_message_id: Some("<issue-2@example.test>"),
            subject: "A second report",
            sender_name: "Another Member",
            sender_address: "another@example.test",
            received_at: 1_786_730_100,
            web_url: "https://outlook.office.com/mail/inbox/id/AAMk-message-2",
            body_text: "The same form fails for me.",
            attachments: &[],
        };
        let imported = store
            .import_email_messages(
                &[first, second],
                &EmailTaskDraft {
                    title: "Fix both reported form failures",
                    description: "Two people reported the same outcome.",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Ready,
                },
            )
            .unwrap();
        for state in [TaskState::Active, TaskState::Review, TaskState::Completed] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .record_task_deployment(imported.task.id, "production", "release-43", 1_786_730_200)
            .unwrap();
        let draft = store
            .prepare_email_reply(imported.task.id, "Thank you. The shared issue is fixed.")
            .unwrap();
        assert_eq!(draft.targets.len(), 2);
        assert!(
            draft
                .targets
                .iter()
                .all(|target| target.state == EmailReplyState::Draft)
        );
        store.queue_email_reply(&draft.id).unwrap();

        let first_target = store.claim_email_reply().unwrap().unwrap();
        assert_eq!(first_target.message_id, "AAMk-message-1");
        let partial = store
            .complete_email_reply(&first_target.target_id, "provider-reply-1")
            .unwrap();
        assert_eq!(partial.state, EmailReplyState::Queued);

        let second_target = store.claim_email_reply().unwrap().unwrap();
        assert_eq!(second_target.message_id, "AAMk-message-2");
        let uncertain = store
            .fail_email_reply(
                &second_target.target_id,
                &EmailReplyFailure::Uncertain("connection ended after send".into()),
            )
            .unwrap();
        assert_eq!(uncertain.state, EmailReplyState::Uncertain);
        assert_eq!(
            uncertain
                .targets
                .iter()
                .filter(|target| target.state == EmailReplyState::Delivered)
                .count(),
            1
        );
        assert_eq!(
            uncertain
                .targets
                .iter()
                .filter(|target| target.state == EmailReplyState::Uncertain)
                .count(),
            1
        );

        let retried = store.retry_uncertain_email_reply(&draft.id).unwrap();
        assert_eq!(retried.state, EmailReplyState::Queued);
        let retry_target = store.claim_email_reply().unwrap().unwrap();
        assert_eq!(retry_target.target_id, second_target.target_id);
        let delivered = store
            .complete_email_reply(&retry_target.target_id, "provider-reply-2")
            .unwrap();
        assert_eq!(delivered.state, EmailReplyState::Delivered);
        assert!(
            delivered
                .targets
                .iter()
                .all(|target| target.state == EmailReplyState::Delivered)
        );
    }

    #[test]
    fn schema_v40_email_sources_migrate_without_losing_tasks_or_attachments() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Legacy email task", "email://inbox")
            .unwrap();
        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch(
            // All three, IF EXISTS: one old trigger became two when the evidence rule moved to the send.
            "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment; DROP TRIGGER IF EXISTS email_reply_draft_requires_finished_work; DROP TRIGGER IF EXISTS email_reply_send_requires_evidence;
             DROP INDEX email_reply_delivery_queue;
             DROP INDEX email_messages_by_task;
             DROP INDEX email_messages_by_conversation;
             DROP INDEX email_attachments_by_task;
             DROP TABLE email_reply_targets;
             DROP TABLE email_reply_deliveries;
             DROP TABLE email_task_attachments;
             DROP TABLE email_message_links;
             CREATE TABLE email_message_links (
                 task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
                 integration_id TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 conversation_id TEXT NOT NULL,
                 internet_message_id TEXT,
                 sender_name TEXT NOT NULL,
                 sender_address TEXT NOT NULL,
                 received_at INTEGER NOT NULL,
                 web_url TEXT NOT NULL,
                 imported_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 UNIQUE (integration_id, message_id)
             );
             CREATE INDEX email_messages_by_conversation
                 ON email_message_links(integration_id, conversation_id, received_at DESC);
             CREATE TABLE email_task_attachments (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES email_message_links(task_id) ON DELETE CASCADE,
                 storage_name TEXT NOT NULL,
                 display_name TEXT NOT NULL,
                 media_type TEXT NOT NULL,
                 byte_size INTEGER NOT NULL CHECK (byte_size > 0),
                 is_inline INTEGER NOT NULL CHECK (is_inline IN (0,1)),
                 content_id TEXT,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 UNIQUE (task_id, storage_name)
             );
             CREATE TABLE email_reply_deliveries (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL UNIQUE REFERENCES email_message_links(task_id) ON DELETE CASCADE,
                 body TEXT NOT NULL,
                 state TEXT NOT NULL,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 attempts INTEGER NOT NULL DEFAULT 0,
                 available_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 attempted_at INTEGER,
                 delivered_at INTEGER,
                 provider_reply_id TEXT,
                 last_error TEXT,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX email_reply_delivery_queue
                 ON email_reply_deliveries(state, available_at, created_at);",
        ).unwrap();
        transaction
            .execute(
                "INSERT INTO email_message_links (
                 task_id, integration_id, message_id, conversation_id, internet_message_id,
                 sender_name, sender_address, received_at, web_url
             ) VALUES (?1, 'operator-outlook', 'legacy-message', 'legacy-thread',
                       '<legacy@example.test>', 'Reporter', 'reporter@example.test',
                       1786730000, 'https://outlook.office.com/mail/legacy-message')",
                [task.id.to_string()],
            )
            .unwrap();
        transaction.execute(
            "INSERT INTO email_task_attachments (
                 id, task_id, storage_name, display_name, media_type, byte_size, is_inline
             ) VALUES ('attachment-1', ?1, 'sha256-screen.png', 'screen.png', 'image/png', 1024, 0)",
            [task.id.to_string()],
        ).unwrap();
        transaction
            .execute(
                "INSERT INTO email_reply_deliveries (
                 id, task_id, body, state, idempotency_key
             ) VALUES ('reply-1', ?1, 'A preserved draft.', 'draft', 'email-reply:legacy')",
                [task.id.to_string()],
            )
            .unwrap();
        migrate_email_multi_source(&transaction).unwrap();
        migrate_email_reply_targets(&transaction).unwrap();
        transaction.commit().unwrap();
        drop(connection);

        let sources = store.email_task_links_for_task(task.id).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, task.id.to_string());
        assert_eq!(sources[0].message_id, "legacy-message");
        assert_eq!(sources[0].attachments[0].display_name, "screen.png");
        assert_eq!(
            store.email_reply_for_task(task.id).unwrap().unwrap().body,
            "A preserved draft."
        );
        assert_eq!(store.get_task(task.id).unwrap().title, "Legacy email task");
    }

    #[test]
    fn resolution_reply_requires_completed_deployed_work_and_delivers_once() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        // Work nobody has finished cannot be answered yet, and the refusal says
        // so in its own words rather than talking about deployments.
        assert!(matches!(
            store.prepare_email_reply(imported.task.id, "The issue is fixed and available now."),
            Err(TaskStoreError::EmailDraftNotReady)
        ));
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        // THE DRAFT IS NOW ALLOWED WITH NO EVIDENCE AT ALL, and that is the
        // point of the change. Writing is private and reversible. The operator
        // was being handed a blank box because the worker's only window to write
        // closed before Queen approved the evidence.
        let draft = store
            .prepare_email_reply(
                imported.task.id,
                "Thank you for reporting this. The form now saves your phone number correctly.",
            )
            .unwrap();
        assert_eq!(draft.state, EmailReplyState::Draft);

        // AND THE SEND IS STILL HELD, which is the property that actually
        // protects the person on the other end. Before this change the send
        // tested `state = 'draft'` and nothing else, so it would have gone out.
        assert!(
            matches!(
                store.queue_email_reply(&draft.id),
                Err(TaskStoreError::EmailReplyNotReady)
            ),
            "an unevidenced reply must not be sendable"
        );

        store
            .record_task_deployment(imported.task.id, "production", "release-42", 1_786_730_100)
            .unwrap();
        assert!(matches!(
            store.prepare_email_reply(imported.task.id, "Duplicate"),
            Err(TaskStoreError::EmailReplyAlreadyExists)
        ));
        let draft = store
            .update_email_reply_draft(
                imported.task.id,
                "Thank you for reporting this. The deployed form now saves your phone number.",
            )
            .unwrap();
        assert!(draft.body.contains("deployed form"));
        let queued = store.queue_email_reply(&draft.id).unwrap();
        assert_eq!(queued.state, EmailReplyState::Queued);
        let claimed = store.claim_email_reply().unwrap().unwrap();
        assert_eq!(claimed.reply_id, draft.id);
        assert_eq!(claimed.attempts, 1);
        let delivered = store
            .complete_email_reply(&draft.id, "provider-reply-1")
            .unwrap();
        assert_eq!(delivered.state, EmailReplyState::Delivered);
        assert!(store.claim_email_reply().unwrap().is_none());
    }

    #[test]
    fn attachment_bounds_fail_before_any_task_is_written() {
        let store = TaskStore::in_memory().unwrap();
        let attachments = [EmailAttachmentSnapshot {
            storage_name: "unsafe/path.png",
            display_name: "screen.png",
            media_type: "image/png",
            byte_size: 1_024,
            inline: false,
            content_id: None,
        }];
        assert!(matches!(
            store.import_email_message(&message(&attachments), TaskPriority::Normal),
            Err(TaskStoreError::InvalidEmailAttachment)
        ));
        assert!(store.list_tasks().unwrap().is_empty());
    }

    #[test]
    fn uncertain_delivery_never_replays_without_operator_retry() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .record_task_deployment(imported.task.id, "production", "release-42", 1_786_730_100)
            .unwrap();
        let draft = store
            .prepare_email_reply(imported.task.id, "The reported issue is now fixed.")
            .unwrap();
        store.queue_email_reply(&draft.id).unwrap();
        store.claim_email_reply().unwrap().unwrap();
        let uncertain = store
            .fail_email_reply(
                &draft.id,
                &EmailReplyFailure::Uncertain("connection ended after send".into()),
            )
            .unwrap();
        assert_eq!(uncertain.state, EmailReplyState::Uncertain);
        assert!(store.claim_email_reply().unwrap().is_none());
        assert_eq!(
            store.retry_uncertain_email_reply(&draft.id).unwrap().state,
            EmailReplyState::Queued
        );
    }

    /// The worker that did the work is the one that knows where it is running
    /// and what to tell the person who wrote in — and it can only report
    /// Active, Blocked or Review for its own task. Gating both acts on
    /// `completed` therefore made them unreachable by that worker, and the
    /// reply fell to whoever closed the task later. That is how the person who
    /// wrote in ended up waiting while the operator drafted it by hand.
    #[test]
    fn a_worker_can_record_where_its_work_runs_and_answer_from_review() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(imported.task.id, state).unwrap();
        }

        // Handed to review, not completed: the furthest a worker can take it.
        store
            .record_task_deployment(imported.task.id, "production", "release-51", 1_786_730_300)
            .unwrap();
        let draft = store
            .prepare_email_reply(
                imported.task.id,
                "Thank you for reporting this. Your phone number is kept now when you press Save.",
            )
            .unwrap();

        assert_eq!(draft.state, EmailReplyState::Draft);
    }

    /// Deployment evidence still means something. Work nobody has finished has
    /// nowhere to be running.
    #[test]
    fn work_still_in_progress_has_no_deployment_to_record() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        assert!(matches!(
            store.record_task_deployment(
                imported.task.id,
                "production",
                "release-51",
                1_786_730_300
            ),
            // Names the ordering rule rather than blaming the reference. The
            // old answer, "evidence is invalid", was returned for a
            // well-formed reference recorded a moment too early, and cost two
            // failed attempts and a bug report filed against the wrong thing.
            Err(TaskStoreError::DeploymentEvidenceTooEarly)
        ));

        // And it is accepted the moment the work is handed off.
        store
            .transition_task(imported.task.id, TaskState::Review)
            .unwrap();
        assert!(
            store
                .record_task_deployment(imported.task.id, "production", "release-51", 1_786_730_300)
                .is_ok()
        );
    }

    /// The operator's screenshot: "Send this reply to 5 original threads?"
    /// listing Bradford Schleifer five times, one per merged message. Merging
    /// is what made it one piece of work; answering it five times undoes that
    /// in the place it shows most — their inbox.
    #[test]
    fn a_ticket_merged_from_one_person_is_answered_once() {
        let store = TaskStore::in_memory().unwrap();
        // Five messages from one person, imported as one merged task.
        let ids = ["AAMk-1", "AAMk-2", "AAMk-3", "AAMk-4", "AAMk-5"];
        let merged: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, message_id)| {
                let mut snapshot = message(&[]);
                snapshot.message_id = message_id;
                snapshot.internet_message_id = None;
                snapshot.received_at = 1_786_730_000 + i64::try_from(index).unwrap_or(0) * 100;
                snapshot
            })
            .collect();
        let imported = store
            .import_email_messages(
                &merged,
                &EmailTaskDraft {
                    title: "Re: Adjustment Request",
                    description: "",
                    priority: TaskPriority::Normal,
                    worker_id: None,
                    state: TaskState::Draft,
                },
            )
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .record_task_deployment(imported.task.id, "production", "release-60", 1_786_730_500)
            .unwrap();

        store
            .prepare_email_reply(imported.task.id, "Thank you — this is fixed and released.")
            .unwrap();

        let reply = store
            .email_reply_for_task(imported.task.id)
            .unwrap()
            .unwrap();
        assert_eq!(reply.targets.len(), 1, "one person, one reply");
    }

    /// Work that legitimately shipped nothing can still answer the person who
    /// asked.
    ///
    /// Found on a real task: someone wrote in with a question, a worker
    /// established that no change was needed, and Queen approved a
    /// no-deployment exemption — "Question, not a change; nothing was built or
    /// shipped". Closing that way is correct and is exactly what the exemption
    /// is for.
    ///
    /// But the reply trigger demanded a row in `task_deployments`, so the answer
    /// could not even be created. The worker's own note said "Answer drafted
    /// for Ryan" and there was nowhere to put it. The operator was then sent to
    /// a form asking them to record a deployment that did not exist and should
    /// not exist — against their own ruling that the worker verifies what is
    /// running and they only review the words.
    /// A refusal that only the refused worker ever saw is how this defect hid.
    ///
    /// The worker wrote the reply, the gate refused it, the turn ended, and the
    /// board recorded nothing. The task then completed and the operator was
    /// handed an empty textarea on a card that said "No reply has been written"
    /// — accurate, useless, and silent about the fact that a reply HAD been
    /// written and thrown away.
    /// A task with no email thread says so, instead of blaming its own state.
    ///
    /// Hit for real: a completed task was refused with "an email reply can be
    /// drafted once the task is in review or completed; this task is neither"
    /// while it was sitting in completed. The gate joined onto the email links,
    /// so "there is nothing to reply to" came out wearing the state error's
    /// words. Same shape as the subject-length message that reported a bad task
    /// TITLE when the email SUBJECT was being validated.
    #[test]
    fn a_task_that_never_came_from_an_email_says_that_rather_than_blaming_its_state() {
        let store = TaskStore::in_memory().unwrap();
        let ordinary = store
            .create_task("Ship the thing", "/projects/app")
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(ordinary.id, state).unwrap();
        }

        // In review, so the state precondition is satisfied and cannot be the
        // reason. The only thing missing is an email thread.
        assert!(matches!(
            store.prepare_email_reply(ordinary.id, "Answering nobody."),
            Err(TaskStoreError::TaskHasNoEmailThread)
        ));
    }

    #[test]
    fn a_refused_draft_is_recorded_on_the_board_rather_than_dying_with_the_turn() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();

        // Still being worked, so a reply is genuinely premature.
        assert!(matches!(
            store.prepare_email_reply(imported.task.id, "An answer written too early."),
            Err(TaskStoreError::EmailDraftNotReady)
        ));

        let activity = store.list_task_activity(imported.task.id, 50).unwrap();
        let trace = activity
            .events
            .iter()
            .find(|event| event.note.contains("refused"))
            .expect("the refusal is on the board");
        assert!(
            trace.note.contains("review or completed"),
            "the trace says what was missing: {}",
            trace.note
        );
    }

    #[test]
    fn an_approved_no_deployment_exemption_lets_the_reply_be_written() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .claim_completion_exemption(imported.task.id, "Question, not a change", None, 1_000)
            .unwrap();

        // THE WORKER CAN WRITE IT NOW, on a claim nobody has approved yet. That
        // is the whole fix: this is the moment a worker actually has, and it
        // used to be the moment it was refused.
        let reply = store
            .prepare_email_reply(imported.task.id, "Answering the question.")
            .unwrap();
        assert_eq!(reply.body, "Answering the question.");

        // A CLAIM ALONE IS STILL NOT EVIDENCE, and must not open the SEND —
        // that is the same rule that governs closing the task.
        let refused = store.queue_email_reply(&reply.id).unwrap_err().to_string();

        // AND THE REFUSAL NAMES WHAT IS ACTUALLY MISSING. The old wording,
        // "requires completed and deployed work", was true and read as a
        // SEQUENCING instruction: finish, deploy, come back. Two sessions hit it
        // on the same task hours apart, both concluded they had arrived too
        // early, and neither checked what the gate tested. Ryan Denee waited
        // eleven days on a correct report because of that sentence.
        assert!(
            refused.contains("approved no-deployment exemption"),
            "{refused}"
        );
        assert!(refused.contains("recorded deployment"), "{refused}");

        // THE DRAFT SURVIVES THE REFUSED SEND. A worker's words are not thrown
        // away because Queen has not signed off yet; that is what left the
        // operator with a blank box.
        assert_eq!(
            store
                .email_reply_for_task(imported.task.id)
                .unwrap()
                .unwrap()
                .body,
            "Answering the question."
        );

        store
            .approve_completion_exemption(imported.task.id, "queen", "Read the handoff.", 1_100)
            .unwrap();

        // And with the exemption approved, the same draft sends.
        assert_eq!(
            store.queue_email_reply(&reply.id).unwrap().state,
            EmailReplyState::Queued
        );
    }

    /// A send that failed is reported as failed, and can still be retried.
    ///
    /// Originally this asserted that a cancelled reply vanishes from the queue,
    /// because a cancelled row blocked every future attempt and the card would
    /// have nagged forever. Both halves have since changed: `prepare_email_reply`
    /// clears a cancelled reply, and not-found turned out to mean the message
    /// MOVED rather than was deleted. Vanishing is now the wrong behaviour — it
    /// is what let seventeen undelivered replies look sent.
    #[test]
    fn a_send_that_failed_is_reported_with_its_cause_and_can_be_retried() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        store
            .record_task_deployment(imported.task.id, "production", "release-70", 1_786_730_100)
            .unwrap();
        let draft = store
            .prepare_email_reply(imported.task.id, "Thank you — this is fixed.")
            .unwrap();

        // While it is a live draft, someone genuinely is waiting.
        let waiting = store.completed_email_tasks_awaiting_a_reply().unwrap();
        assert_eq!(waiting.len(), 1);
        assert!(waiting[0].drafted, "a live draft is written");

        // The thread is gone, so sending fails permanently.
        store.queue_email_reply(&draft.id).unwrap();
        store.claim_email_reply().unwrap().unwrap();
        store
            .fail_email_reply(
                &draft.id,
                &EmailReplyFailure::Permanent("The email message was not found".into()),
            )
            .unwrap();

        // A FAILED SEND IS REPORTED, carrying why. This used to assert the
        // opposite — that a cancelled reply disappears — and the justification
        // has since expired twice over.
        //
        // It rested on the message being DELETED. It was not: a Graph id is
        // folder-scoped and changes when a message MOVES, so filing an email
        // made it unanswerable and "not found" meant moved, not gone. Hiding
        // that cost the operator seventeen replies on 2026-08-25 — they pressed
        // Send, the item left the queue, and they found out by opening Outlook
        // and seeing nothing.
        //
        // Its second reason, that a cancelled row blocked every future attempt,
        // was fixed separately: prepare_email_reply now clears a cancelled reply
        // before writing a new one, which is exercised below. So the dead end
        // this was avoiding no longer exists, and the silence it bought is the
        // only thing left.
        let waiting = store.completed_email_tasks_awaiting_a_reply().unwrap();
        assert_eq!(waiting.len(), 1, "a send that failed is still unanswered");
        assert_eq!(
            waiting[0].delivery_failure.as_deref(),
            Some("The email message was not found"),
            "and it carries the cause, rather than looking like nobody has written one"
        );
        // AND IT CARRIES THE REPLY ITSELF. The body is still on the cancelled
        // row, and this query used to read last_error off that row and the body
        // from nowhere — it looked straight at the draft and took only the bad
        // news. So the queue announced that no reply had been written while
        // four of them, 1.0 to 1.6 KB each, sat unreadable in the database on
        // 2026-08-25. The operator was told the work was gone; pressing Write
        // the reply opened an empty box over the top of it.
        assert_eq!(
            waiting[0].draft_body.as_deref(),
            Some("Thank you — this is fixed."),
            "a failed send must not read as an unwritten reply"
        );
        assert!(
            waiting[0].drafted,
            "and the card must not offer to write one that already exists"
        );

        // And the dead end is no longer permanent: a new reply can be written.
        store
            .prepare_email_reply(imported.task.id, "Trying again on a thread that came back.")
            .unwrap();
        assert_eq!(
            store
                .completed_email_tasks_awaiting_a_reply()
                .unwrap()
                .len(),
            1
        );
    }

    /// Driven by the inputs that FAILED, because the message a stuck caller
    /// sees is the entire defect.
    ///
    /// A worker reported that a bare SHA and a bare URL were rejected as
    /// "task deployment evidence is invalid", and filed against the validator.
    /// The validator never inspected shape. What it rejected was the task's
    /// STATE — the reference was fine and recorded a moment too early — and it
    /// answered with the wrong rule, which is how the report came to be filed
    /// against the wrong thing entirely.
    #[test]
    fn a_refused_deployment_says_which_rule_it_broke() {
        use swarm_domain::{TaskPriority, TaskState};
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task_with_details("Shipped", "", TaskPriority::Normal, "/workspace")
            .unwrap();

        // Too early: the rule broken is the state, and the message says so
        // rather than blaming the evidence.
        let early = store
            .record_task_deployment(task.id, "production", "e99140c", 1_000)
            .expect_err("a task not yet finished cannot carry deployment evidence");
        assert!(
            matches!(early, TaskStoreError::DeploymentEvidenceTooEarly),
            "the state rule must not be reported as invalid evidence: {early}"
        );
        assert!(early.to_string().contains("move this task to review first"));

        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();

        // The shapes the report said were rejected. They are not, and never
        // were: nothing here inspects shape.
        for reference in ["e99140c", "https://github.com/owner/repo/releases/tag/v1"] {
            store
                .record_task_deployment(task.id, "production", reference, 1_000)
                .unwrap_or_else(|error| panic!("{reference} must be accepted: {error}"));
        }

        // What IS rejected, and the refusal now names the limit rather than
        // leaving the caller to guess at a shape that was never checked.
        let empty = store
            .record_task_deployment(task.id, "production", "   ", 1_000)
            .expect_err("an empty reference is not evidence");
        let said = empty.to_string();
        assert!(said.contains("512"), "the limit must be named: {said}");
        assert!(
            said.contains("bare commit") && said.contains("bare URL"),
            "and it must say the shape is not what was wrong: {said}"
        );
    }
}
