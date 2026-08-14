use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{Task, TaskId, TaskPriority, TaskState};
use uuid::Uuid;

use crate::{
    ControlRoomEventKind, TaskStore, TaskStoreError, insert_control_room_event, parse_domain_id,
    validate_description, validate_text,
};

const MAX_SOURCE_ID_BYTES: usize = 512;
const MAX_CONVERSATION_ID_BYTES: usize = 512;
const MAX_INTERNET_MESSAGE_ID_BYTES: usize = 998;
const MAX_SENDER_NAME_BYTES: usize = 320;
const MAX_SENDER_ADDRESS_BYTES: usize = 320;
const MAX_WEB_URL_BYTES: usize = 2_048;
const MAX_EMAIL_ATTACHMENTS: usize = 16;
const MAX_EMAIL_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_EMAIL_ATTACHMENT_TOTAL_BYTES: u64 = 25 * 1024 * 1024;
const MAX_ATTACHMENT_NAME_BYTES: usize = 255;
const MAX_MEDIA_TYPE_BYTES: usize = 127;
const MAX_CONTENT_ID_BYTES: usize = 512;
const MAX_DEPLOYMENT_FIELD_BYTES: usize = 512;
const MAX_EMAIL_REPLY_BYTES: usize = 10_000;
const MAX_PENDING_EMAIL_REPLIES: i64 = 256;

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
    pub created: bool,
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
        Ok(connection.execute(
            "UPDATE email_reply_deliveries
             SET state = 'uncertain', last_error = 'Swarm restarted before delivery was confirmed',
                 updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?)
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
        validate_email_message(message)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(existing_task_id) = transaction
            .query_row(
                "SELECT task_id FROM email_message_links
                 WHERE integration_id = ?1 AND message_id = ?2",
                params![message.integration_id.trim(), message.message_id.trim()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            let task_id = parse_domain_id::<TaskId>(&existing_task_id)?;
            transaction.commit()?;
            drop(connection);
            return Ok(EmailImport {
                task: self.get_task(task_id)?,
                source: self
                    .email_task_link(task_id)?
                    .ok_or(TaskStoreError::EmailSourceNotFound)?,
                created: false,
            });
        }

        let hive_id: String = transaction.query_row(
            "SELECT hive_id FROM local_hive_identity WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        let task_id = TaskId::new();
        let subject = email_task_title(message);
        let body = message.body_text.trim();
        transaction.execute(
            "INSERT INTO tasks (
                 id, hive_id, title, description, priority, workspace, state, position
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'email://inbox', 'draft',
                 COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
            params![
                task_id.to_string(),
                hive_id,
                subject,
                body,
                priority.to_string(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state, note)
             VALUES (?1, 'created', 'draft', 'Imported from email')",
            [task_id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO email_message_links (
                 task_id, integration_id, message_id, conversation_id,
                 internet_message_id, sender_name, sender_address, received_at, web_url
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
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
        for attachment in message.attachments {
            transaction.execute(
                "INSERT INTO email_task_attachments (
                     id, task_id, storage_name, display_name, media_type,
                     byte_size, is_inline, content_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Uuid::now_v7().to_string(),
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
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        Ok(EmailImport {
            task: self.get_task(task_id)?,
            source: self
                .email_task_link(task_id)?
                .ok_or(TaskStoreError::EmailSourceNotFound)?,
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
        let connection = self.connection()?;
        let source = connection
            .query_row(
                "SELECT task_id, integration_id, message_id, conversation_id,
                        internet_message_id, sender_name, sender_address, received_at,
                        web_url, imported_at
                 FROM email_message_links WHERE task_id = ?1",
                [task_id.to_string()],
                email_task_link_from_row,
            )
            .optional()?;
        let Some(mut source) = source else {
            return Ok(None);
        };
        let mut statement = connection.prepare(
            "SELECT storage_name, display_name, media_type, byte_size, is_inline, content_id
             FROM email_task_attachments WHERE task_id = ?1 ORDER BY created_at, id",
        )?;
        source.attachments = statement
            .query_map([task_id.to_string()], email_attachment_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(source))
    }

    /// Lists immutable email sources attached to tasks, newest imports first.
    ///
    /// # Errors
    /// Returns an error when source metadata is corrupt or persistence is unavailable.
    pub fn email_task_links(&self) -> Result<Vec<EmailTaskLink>, TaskStoreError> {
        let connection = self.connection()?;
        let mut sources = {
            let mut statement = connection.prepare(
                "SELECT task_id, integration_id, message_id, conversation_id,
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
             FROM email_task_attachments WHERE task_id = ?1 ORDER BY created_at, id",
        )?;
        for source in &mut sources {
            source.attachments = attachment_statement
                .query_map([source.task_id.to_string()], email_attachment_from_row)?
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
        connection
            .query_row(
                "SELECT id, task_id, body, state, idempotency_key, attempts,
                        available_at, attempted_at, delivered_at, provider_reply_id, last_error
                 FROM email_reply_deliveries WHERE task_id = ?1",
                [task_id.to_string()],
                email_reply_from_row,
            )
            .optional()
            .map_err(Into::into)
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
        let environment = environment.trim();
        let reference = reference.trim();
        if !bounded_text(environment, MAX_DEPLOYMENT_FIELD_BYTES)
            || !bounded_text(reference, MAX_DEPLOYMENT_FIELD_BYTES)
            || deployed_at <= 0
        {
            return Err(TaskStoreError::InvalidTaskDeployment);
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
        if task_state != TaskState::Completed.to_string() {
            return Err(TaskStoreError::InvalidTaskDeployment);
        }
        let id = Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT OR IGNORE INTO task_deployments (
                 id, task_id, environment, reference, deployed_at, approved_by_operator_id
             ) SELECT ?1, ?2, ?3, ?4, ?5, o.id
               FROM local_hive_identity local
               JOIN hives h ON h.id = local.hive_id
               JOIN operators o ON o.id = h.operator_id
               WHERE local.singleton = 1",
            params![id, task_id.to_string(), environment, reference, deployed_at],
        )?;
        let record = transaction.query_row(
            "SELECT id, task_id, environment, reference, deployed_at, recorded_at
             FROM task_deployments
             WHERE task_id = ?1 AND environment = ?2 AND reference = ?3",
            params![task_id.to_string(), environment, reference],
            deployment_from_row,
        )?;
        transaction.commit()?;
        Ok(record)
    }

    /// Creates one reviewed reply draft only after the linked task is completed and deployed.
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
        let ready: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN email_message_links source ON source.task_id = task.id
                 JOIN task_deployments deployment ON deployment.task_id = task.id
                 WHERE task.id = ?1 AND task.state = 'completed'
             )",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if !ready {
            return Err(TaskStoreError::EmailReplyNotReady);
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
        transaction.commit()?;
        Ok(dispatch)
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
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE email_reply_deliveries SET state = 'queued', updated_at = unixepoch()
             WHERE id = ?1 AND state = 'draft'",
            [id],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        email_reply_by_id(&connection, id)?.ok_or(TaskStoreError::InvalidEmailReply)
    }

    /// Claims the oldest due reply for the adapter. Credentials never enter this record.
    ///
    /// # Errors
    /// Returns an error when queue state is corrupt or persistence is unavailable.
    pub fn claim_email_reply(&self) -> Result<Option<EmailReplyDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = transaction
            .query_row(
                "SELECT id FROM email_reply_deliveries
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
            "UPDATE email_reply_deliveries
             SET state = 'dispatching', attempts = attempts + 1,
                 attempted_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?1",
            [&id],
        )?;
        let dispatch =
            email_reply_by_id(&transaction, &id)?.ok_or(TaskStoreError::InvalidEmailReply)?;
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
        let changed = connection.execute(
            "UPDATE email_reply_deliveries
             SET state = 'delivered', provider_reply_id = ?2, delivered_at = unixepoch(),
                 last_error = NULL, updated_at = unixepoch()
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, provider_reply_id.trim()],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        email_reply_by_id(&connection, id)?.ok_or(TaskStoreError::InvalidEmailReply)
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
            "UPDATE email_reply_deliveries
             SET state = 'queued', available_at = unixepoch(),
                 last_error = NULL, updated_at = unixepoch()
             WHERE id = ?1 AND state = 'uncertain' AND attempts < 3",
            [id],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        email_reply_by_id(&connection, id)?.ok_or(TaskStoreError::InvalidEmailReply)
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
        let attempts = connection
            .query_row(
                "SELECT attempts FROM email_reply_deliveries
                 WHERE id = ?1 AND state = 'dispatching'",
                [id],
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
            "UPDATE email_reply_deliveries
             SET state = ?2, available_at = unixepoch() + ?3,
                 last_error = ?4, updated_at = unixepoch()
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, state, retry_delay, error],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        email_reply_by_id(&connection, id)?.ok_or(TaskStoreError::InvalidEmailReply)
    }
}

pub(crate) fn migrate_email_intake(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS email_message_links (
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
         CREATE INDEX IF NOT EXISTS email_messages_by_conversation
             ON email_message_links(integration_id, conversation_id, received_at DESC);
         CREATE TABLE IF NOT EXISTS email_task_attachments (
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
             task_id TEXT NOT NULL UNIQUE REFERENCES email_message_links(task_id) ON DELETE CASCADE,
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
         PRAGMA user_version = 40;",
    )
}

fn validate_email_message(message: &EmailMessageSnapshot<'_>) -> Result<(), TaskStoreError> {
    let subject = email_task_title(message);
    let body = message.body_text.trim();
    validate_text(&subject, "email://inbox")?;
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

fn email_task_title(message: &EmailMessageSnapshot<'_>) -> String {
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
        task_id: parse_domain_id::<TaskId>(&row.get::<_, String>(0)?)?,
        integration_id: row.get(1)?,
        message_id: row.get(2)?,
        conversation_id: row.get(3)?,
        internet_message_id: row.get(4)?,
        sender_name: row.get(5)?,
        sender_address: row.get(6)?,
        received_at: row.get(7)?,
        web_url: row.get(8)?,
        imported_at: row.get(9)?,
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
    })
}

fn email_reply_by_id(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<EmailReplyDispatch>, TaskStoreError> {
    connection
        .query_row(
            "SELECT id, task_id, body, state, idempotency_key, attempts,
                    available_at, attempted_at, delivered_at, provider_reply_id, last_error
             FROM email_reply_deliveries WHERE id = ?1",
            [id],
            email_reply_from_row,
        )
        .optional()
        .map_err(Into::into)
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
    fn resolution_reply_requires_completed_deployed_work_and_delivers_once() {
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(&message(&[]), TaskPriority::Normal)
            .unwrap();
        assert_eq!(
            store
                .prepare_email_reply(imported.task.id, "The issue is fixed and available now.")
                .unwrap_err()
                .to_string(),
            TaskStoreError::EmailReplyNotReady.to_string()
        );
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        assert!(matches!(
            store.prepare_email_reply(imported.task.id, "Still too early"),
            Err(TaskStoreError::EmailReplyNotReady)
        ));
        store
            .record_task_deployment(imported.task.id, "production", "release-42", 1_786_730_100)
            .unwrap();
        let draft = store
            .prepare_email_reply(
                imported.task.id,
                "Thank you for reporting this. The form now saves your phone number correctly.",
            )
            .unwrap();
        assert_eq!(draft.state, EmailReplyState::Draft);
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
        assert_eq!(claimed.id, draft.id);
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
}
