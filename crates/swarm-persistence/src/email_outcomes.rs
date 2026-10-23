//! Closing an email task's reply without Swarm sending it (ADR 0121).
//!
//! The card in Needs you used to offer one way out: send. Some threads need no
//! answer, and some answers the operator wants to finish in Outlook. Both now
//! end the item on the operator's say-so, recorded on the task, and neither
//! sends anything.

use rusqlite::{OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, TaskId};

use super::{TaskStore, TaskStoreError, insert_control_room_event};

/// How an email task's reply was closed, when Swarm did not send it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmailReplyOutcome {
    /// The operator decided no reply was needed.
    NotSent,
    /// The reply was saved as drafts in the operator's Outlook, for them to
    /// finish and send. Swarm cannot see whether they do.
    SavedAsDraft,
}

impl EmailReplyOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::NotSent => "not_sent",
            Self::SavedAsDraft => "saved_as_draft",
        }
    }
}

/// What saving one reply as Outlook drafts needs: the text, and each person it
/// would have gone to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailDraftPlan {
    pub body: String,
    pub targets: Vec<EmailDraftTarget>,
}

/// One person the reply answers, and the message a draft replies to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailDraftTarget {
    pub target_id: String,
    /// The mailbox the message arrived on, which is where the draft goes.
    pub integration_id: String,
    pub message_id: String,
    pub internet_message_id: Option<String>,
    /// A draft already exists for this person, from an earlier attempt that
    /// stopped part-way. It is not made again.
    pub drafted: bool,
}

impl TaskStore {
    /// The reply and its unanswered recipients, for saving as Outlook drafts.
    ///
    /// Only people a send has not already reached: a reply cancelled part-way
    /// through keeps its delivered targets, and drafting those again would
    /// answer someone twice.
    ///
    /// # Errors
    /// Refuses a task that is not a completed email task, a reply that is going
    /// out or whose result is unknown, one already closed, and one with no
    /// written text.
    pub fn email_reply_draft_plan(
        &self,
        task_id: TaskId,
    ) -> Result<EmailDraftPlan, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        ensure_reply_closable(&transaction, task_id)?;
        let reply = transaction
            .query_row(
                "SELECT id, body FROM email_reply_deliveries
                 WHERE task_id = ?1 AND state IN ('draft', 'cancelled') AND trim(body) <> ''",
                [task_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((reply_id, body)) = reply else {
            return Err(TaskStoreError::EmailReplyNotWritten);
        };
        let targets = {
            let mut statement = transaction.prepare(
                "SELECT target.id, source.integration_id, source.message_id,
                        source.internet_message_id,
                        EXISTS(SELECT 1 FROM email_reply_outlook_drafts draft
                               WHERE draft.target_id = target.id)
                 FROM email_reply_targets target
                 JOIN email_message_links source ON source.id = target.source_id
                 WHERE target.reply_id = ?1 AND target.state <> 'delivered'
                 ORDER BY target.created_at, target.id",
            )?;
            statement
                .query_map([&reply_id], |row| {
                    Ok(EmailDraftTarget {
                        target_id: row.get(0)?,
                        integration_id: row.get(1)?,
                        message_id: row.get(2)?,
                        internet_message_id: row.get(3)?,
                        drafted: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if targets.is_empty() {
            return Err(TaskStoreError::EmailReplyNotWritten);
        }
        transaction.commit()?;
        Ok(EmailDraftPlan { body, targets })
    }

    /// Records one Outlook draft as made, so a retry after a partial failure
    /// skips it.
    ///
    /// # Errors
    /// Rejects an unbounded draft identity or link, or unavailable persistence.
    pub fn record_email_reply_outlook_draft(
        &self,
        target_id: &str,
        draft_message_id: &str,
        web_link: Option<&str>,
    ) -> Result<(), TaskStoreError> {
        if draft_message_id.is_empty()
            || draft_message_id.len() > 1_024
            || web_link.is_some_and(|link| link.len() > 4_096)
        {
            return Err(TaskStoreError::InvalidEmailReply);
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO email_reply_outlook_drafts (target_id, draft_message_id, web_link)
             VALUES (?1, ?2, ?3)",
            params![target_id, draft_message_id, web_link],
        )?;
        Ok(())
    }

    /// Takes an email task out of Needs you without Swarm sending the reply.
    ///
    /// Returns false when it was already closed, so a second press changes
    /// nothing.
    ///
    /// # Errors
    /// Refuses a task that is not a completed email task, a reply that is going
    /// out or whose result is unknown, and saving as drafts before every
    /// remaining recipient has one.
    pub fn close_email_reply(
        &self,
        task_id: TaskId,
        outcome: EmailReplyOutcome,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        match ensure_reply_closable(&transaction, task_id) {
            Err(TaskStoreError::EmailReplyClosed) => return Ok(false),
            other => other?,
        }
        let note = match outcome {
            EmailReplyOutcome::NotSent => {
                "Closed without a reply: the operator chose not to send one.".to_owned()
            }
            EmailReplyOutcome::SavedAsDraft => {
                let (drafted, undrafted): (i64, i64) = transaction.query_row(
                    "SELECT
                         COUNT(*) FILTER (WHERE EXISTS (
                             SELECT 1 FROM email_reply_outlook_drafts draft
                             WHERE draft.target_id = target.id)),
                         COUNT(*) FILTER (WHERE NOT EXISTS (
                             SELECT 1 FROM email_reply_outlook_drafts draft
                             WHERE draft.target_id = target.id))
                     FROM email_reply_targets target
                     JOIN email_reply_deliveries reply ON reply.id = target.reply_id
                     WHERE reply.task_id = ?1 AND target.state <> 'delivered'",
                    [task_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                if drafted == 0 || undrafted > 0 {
                    return Err(TaskStoreError::IntegrityFailure(
                        "every remaining recipient needs an Outlook draft before the reply closes"
                            .into(),
                    ));
                }
                let noun = if drafted == 1 { "draft" } else { "drafts" };
                format!(
                    "Saved the reply as {drafted} {noun} in Outlook for the operator to send. Swarm did not send it."
                )
            }
        };
        transaction.execute(
            "INSERT INTO email_reply_outcomes (task_id, outcome) VALUES (?1, ?2)",
            params![task_id.to_string(), outcome.as_str()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind)
             VALUES (?1, 'details_updated', ?2, 'operator')",
            params![task_id.to_string(), note],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(true)
    }
}

/// The checks closing and drafting share.
///
/// ⚠️ IN FLIGHT REFUSES, because both actions would otherwise race a send: a
/// reply going out, or one Microsoft may already have accepted, closed as "not
/// sent" says something untrue, and drafted again answers the person twice.
fn ensure_reply_closable(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> Result<(), TaskStoreError> {
    let task = transaction
        .query_row(
            "SELECT state, removed_at IS NOT NULL,
                    EXISTS(SELECT 1 FROM email_message_links link WHERE link.task_id = tasks.id)
             FROM tasks WHERE id = ?1",
            [task_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((state, removed, from_email)) = task else {
        return Err(TaskStoreError::NotFound);
    };
    if removed {
        return Err(TaskStoreError::NotFound);
    }
    if !from_email {
        return Err(TaskStoreError::TaskHasNoEmailThread);
    }
    if state != "completed" {
        return Err(TaskStoreError::EmailDraftNotReady);
    }
    let closed: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM email_reply_outcomes WHERE task_id = ?1)",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    if closed {
        return Err(TaskStoreError::EmailReplyClosed);
    }
    let in_flight: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM email_reply_targets target
             JOIN email_reply_deliveries reply ON reply.id = target.reply_id
             WHERE reply.task_id = ?1 AND target.state IN ('queued', 'dispatching', 'uncertain'))",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    if in_flight {
        return Err(TaskStoreError::EmailReplyInFlight);
    }
    Ok(())
}

/// Adds the two tables that record a reply Swarm did not send (ADR 0121).
pub(super) fn migrate_email_reply_outcomes(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS email_reply_outcomes (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             outcome TEXT NOT NULL CHECK (outcome IN ('not_sent', 'saved_as_draft')),
             decided_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE TABLE IF NOT EXISTS email_reply_outlook_drafts (
             target_id TEXT PRIMARY KEY REFERENCES email_reply_targets(id) ON DELETE CASCADE,
             draft_message_id TEXT NOT NULL
                 CHECK (length(draft_message_id) BETWEEN 1 AND 1024),
             web_link TEXT CHECK (web_link IS NULL OR length(web_link) <= 4096),
             created_at INTEGER NOT NULL DEFAULT (unixepoch())
         );",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

#[cfg(test)]
mod tests {
    use swarm_domain::{TaskPriority, TaskState};

    use super::*;
    use crate::EmailMessageSnapshot;

    fn message() -> EmailMessageSnapshot<'static> {
        EmailMessageSnapshot {
            integration_id: "operator-outlook",
            message_id: "AAMk-message-1",
            conversation_id: "AAQk-conversation-1",
            internet_message_id: Some("<issue-1@example.test>"),
            subject: "New website: advertising implications",
            sender_name: "A Member",
            sender_address: "member@example.test",
            received_at: 1_786_730_000,
            web_url: "https://outlook.office.com/mail/inbox/id/AAMk-message-1",
            body_text: "Will the page addresses change?",
            attachments: &[],
        }
    }

    /// A completed email task, with a written reply when `reply` is given.
    fn completed_email_task(store: &TaskStore, reply: Option<&str>) -> (TaskId, Option<String>) {
        let imported = store
            .import_email_message(&message(), TaskPriority::Normal)
            .unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, state).unwrap();
        }
        let draft = reply.map(|body| {
            store
                .record_task_deployment(imported.task.id, "production", "release-9", 1_786_730_200)
                .unwrap();
            store
                .prepare_email_reply(imported.task.id, body)
                .unwrap()
                .id
        });
        (imported.task.id, draft)
    }

    fn needs_you_email_subjects(store: &TaskStore) -> usize {
        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        crate::attention::needs_you_subjects(&transaction, 2_000_000_000)
            .unwrap()
            .into_iter()
            .filter(|subject| subject.kind == "email_reply")
            .count()
    }

    /// The operator's case: "Sometimes we don't need to send an email." The
    /// card used to offer only Send, so the item could never leave Needs you.
    #[test]
    fn an_email_that_needs_no_reply_leaves_needs_you_without_anything_sent() {
        let store = TaskStore::in_memory().unwrap();
        let (task_id, _) = completed_email_task(&store, Some("Short answers for now."));
        assert_eq!(
            store
                .completed_email_tasks_awaiting_a_reply()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(needs_you_email_subjects(&store), 1);

        assert!(
            store
                .close_email_reply(task_id, EmailReplyOutcome::NotSent)
                .unwrap()
        );

        assert!(
            store
                .completed_email_tasks_awaiting_a_reply()
                .unwrap()
                .is_empty()
        );
        assert_eq!(needs_you_email_subjects(&store), 0);
        assert!(
            store.claim_email_reply().unwrap().is_none(),
            "nothing was queued to send"
        );
        assert!(
            !store
                .close_email_reply(task_id, EmailReplyOutcome::NotSent)
                .unwrap(),
            "a second press changes nothing"
        );
    }

    /// No reply written is not a reason to keep the item: the operator may
    /// decide there is nothing to say.
    #[test]
    fn an_email_with_no_reply_written_can_still_be_closed_but_not_drafted() {
        let store = TaskStore::in_memory().unwrap();
        let (task_id, _) = completed_email_task(&store, None);

        assert!(matches!(
            store.email_reply_draft_plan(task_id),
            Err(TaskStoreError::EmailReplyNotWritten)
        ));
        assert!(
            store
                .close_email_reply(task_id, EmailReplyOutcome::NotSent)
                .unwrap()
        );
    }

    /// A reply already going out cannot be called "not sent", and drafting it
    /// would answer the person twice.
    #[test]
    fn a_reply_on_its_way_cannot_be_closed_or_drafted() {
        let store = TaskStore::in_memory().unwrap();
        let (task_id, draft) = completed_email_task(&store, Some("Short answers for now."));
        store.queue_email_reply(&draft.unwrap()).unwrap();

        assert!(matches!(
            store.close_email_reply(task_id, EmailReplyOutcome::NotSent),
            Err(TaskStoreError::EmailReplyInFlight)
        ));
        assert!(matches!(
            store.email_reply_draft_plan(task_id),
            Err(TaskStoreError::EmailReplyInFlight)
        ));
    }

    /// Saving as drafts closes only once every remaining person has one, and
    /// a reply saved in Outlook is not then sent from Swarm as well.
    #[test]
    fn a_reply_saved_as_outlook_drafts_closes_once_and_is_not_sent_afterwards() {
        let store = TaskStore::in_memory().unwrap();
        let (task_id, draft) = completed_email_task(&store, Some("Short answers for now."));

        let plan = store.email_reply_draft_plan(task_id).unwrap();
        assert_eq!(plan.body, "Short answers for now.");
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].message_id, "AAMk-message-1");
        assert_eq!(plan.targets[0].integration_id, "operator-outlook");
        assert!(!plan.targets[0].drafted);
        assert!(
            store
                .close_email_reply(task_id, EmailReplyOutcome::SavedAsDraft)
                .is_err(),
            "no draft exists yet"
        );

        store
            .record_email_reply_outlook_draft(
                &plan.targets[0].target_id,
                "AAMk-draft-1",
                Some("https://outlook.office.com/mail/drafts/1"),
            )
            .unwrap();
        assert!(store.email_reply_draft_plan(task_id).unwrap().targets[0].drafted);
        assert!(
            store
                .close_email_reply(task_id, EmailReplyOutcome::SavedAsDraft)
                .unwrap()
        );

        assert!(
            store
                .completed_email_tasks_awaiting_a_reply()
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            store.queue_email_reply(&draft.unwrap()),
            Err(TaskStoreError::EmailReplyClosed)
        ));
    }
}
