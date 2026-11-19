//! Jira replies the operator posts from Needs you (ADR 0177).
//!
//! A worker that answered a ticket DRAFTS the reply to whoever raised it. It
//! does not post it, and it does not ask the operator in a decision and then
//! post it afterwards: that relay through the worker is what kept a reply the
//! operator had approved off the ticket for two minutes. The operator reads the
//! draft in Needs you, edits it if they want, and presses. That press queues
//! the comment through the ordinary Jira comment delivery and sends it at once.
//!
//! One draft per task. A later draft replaces an earlier one. A posted draft
//! stays on its card until Jira has the comment, so a refusal is shown where
//! the operator pressed rather than only on the task.

use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{ControlRoomEventKind, TaskId, WorkerId};

use crate::jira::{MAX_COMMENT_BYTES, queue_comment_in};
use crate::{TaskStore, TaskStoreError, insert_control_room_event, parse_domain_id};

/// The most replies one read returns. Each is one ticket, so a Hive with more
/// than this waiting has a backlog the cards already show.
const MAX_JIRA_REPLIES_LISTED: i64 = 200;

/// A draft still waiting on the operator: never posted, or posted and not yet
/// taken by Jira. One condition, shared by the cards, the badge and the held
/// resolutions it stands in for, so the three cannot disagree.
macro_rules! jira_reply_owed {
    () => {
        "(draft.posted_at IS NULL OR EXISTS (
             SELECT 1 FROM jira_comment_deliveries comment
             WHERE comment.id = draft.comment_id AND comment.state <> 'delivered'))"
    };
}
pub(crate) use jira_reply_owed;

/// Adds the draft table (schema 231).
pub(crate) fn migrate_jira_reply_drafts(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_reply_drafts (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 4000),
             drafted_by TEXT NOT NULL,
             drafted_at INTEGER NOT NULL,
             comment_id TEXT REFERENCES jira_comment_deliveries(id) ON DELETE SET NULL,
             posted_at INTEGER,
             resolve INTEGER NOT NULL DEFAULT 0 CHECK (resolve IN (0, 1))
         ) WITHOUT ROWID;",
    )?;
    // Who raised each linked ticket, so the card can say who the reply is
    // for. Filled by the next sync; until then the card says "the person who
    // raised it".
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('jira_issue_links')
                       WHERE name = 'jira_reporter_name')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        transaction
            .execute_batch("ALTER TABLE jira_issue_links ADD COLUMN jira_reporter_name TEXT;")?;
    }
    transaction.pragma_update(None, "user_version", version)
}

/// A reply drafted for a Jira ticket that waits on the operator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JiraReplyAwaiting {
    pub task_id: TaskId,
    pub issue_key: String,
    pub project_name: String,
    pub title: String,
    pub worker_name: Option<String>,
    /// Who raised the ticket in Jira, as its reporter: the person the reply
    /// answers. None until a sync has read it.
    pub requester_name: Option<String>,
    /// What will be posted: the worker's draft, or the operator's edit of it
    /// once they pressed.
    pub body: String,
    pub drafted_at: i64,
    /// The task's Jira resolution waits for the operator too (ADR 0120). The
    /// card then offers to post and resolve in one press, and stands in for
    /// the resolution's own card.
    pub resolution_held: bool,
    /// The task is complete and its resolution is queued for Jira, waiting
    /// only for this reply to land or be dropped.
    pub resolution_waiting: bool,
    /// Set once the operator pressed and Jira has not taken it yet.
    pub posted: Option<JiraReplyPosted>,
}

/// What one press queued.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostedJiraReply {
    pub comment_id: String,
    /// The ticket resolves once Jira has this reply: the operator asked to
    /// resolve a held resolution, or a completion is waiting on the reply.
    pub resolution_follows: bool,
}

/// Where a posted reply stands while Jira has not taken it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JiraReplyPosted {
    /// `queued`, `dispatching`, `conflict` (Jira refused it, or it could not
    /// be reached three times) or `uncertain` (Swarm stopped mid-send).
    pub state: String,
    pub error: Option<String>,
    pub posted_at: i64,
    /// The operator asked to resolve the ticket too. The held resolution is
    /// released when Jira takes the reply, and not before.
    pub resolve: bool,
}

impl TaskStore {
    /// Records the reply a worker drafted for a Jira-linked task, replacing
    /// any earlier draft.
    ///
    /// # Errors
    /// `InvalidJiraComment` for an empty, oversized or NUL-bearing body;
    /// `NotFound` when the task has no Jira issue; `JiraReplyPosting` while the
    /// operator's press of the last draft is on its way; a persistence error.
    pub fn draft_jira_reply(
        &self,
        worker_id: WorkerId,
        task_id: TaskId,
        body: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let body = valid_body(body)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let issue_key = issue_key(&transaction, task_id)?.ok_or(TaskStoreError::NotFound)?;
        // The operator's last press, if it has not landed. One on its way is
        // theirs and is not replaced under them. One Jira refused, or that
        // Swarm lost track of, is dropped with the draft it posted, so the
        // task's retry cannot send the old words beside the new ones.
        let pressed = transaction
            .query_row(
                "SELECT comment.id, comment.state FROM jira_reply_drafts draft
                 JOIN jira_comment_deliveries comment ON comment.id = draft.comment_id
                 WHERE draft.task_id = ?1",
                [task_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let failed = match pressed {
            Some((_, state)) if state == "queued" || state == "dispatching" => {
                return Err(TaskStoreError::JiraReplyPosting);
            }
            Some((id, state)) if state == "conflict" || state == "uncertain" => Some(id),
            _ => None,
        };
        transaction.execute(
            "INSERT INTO jira_reply_drafts (task_id, body, drafted_by, drafted_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(task_id) DO UPDATE SET
                 body = excluded.body, drafted_by = excluded.drafted_by,
                 drafted_at = excluded.drafted_at, comment_id = NULL, posted_at = NULL,
                 resolve = 0",
            params![task_id.to_string(), body, worker_id.to_string(), now],
        )?;
        if let Some(failed) = failed {
            transaction.execute(
                "DELETE FROM jira_comment_deliveries WHERE id = ?1",
                [failed],
            )?;
        }
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
             VALUES (?1, 'details_updated', ?2, 'worker', ?3)",
            params![
                task_id.to_string(),
                format!(
                    "Drafted a reply for {issue_key}. It waits in Needs you for the operator to post it."
                ),
                worker_id.to_string(),
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replies waiting on the operator, oldest first. The same rows the
    /// Needs-you count reads.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn jira_replies_awaiting(&self) -> Result<Vec<JiraReplyAwaiting>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(concat!(
            "SELECT task.id, link.issue_key, COALESCE(binding.project_name, ''), task.title,
                    worker.name, draft.body, draft.drafted_at,
                    EXISTS (SELECT 1 FROM jira_transition_deliveries held
                            WHERE held.task_id = task.id AND held.state = 'held'),
                    comment.state, comment.last_error, draft.posted_at, draft.resolve,
                    EXISTS (SELECT 1 FROM jira_transition_deliveries waiting
                            WHERE waiting.task_id = task.id AND waiting.state = 'queued'
                              AND waiting.target_task_state = 'completed'),
                    link.jira_reporter_name
             FROM jira_reply_drafts draft
             JOIN tasks task ON task.id = draft.task_id
             JOIN jira_issue_links link ON link.task_id = task.id
             LEFT JOIN jira_project_bindings binding ON binding.id = link.binding_id
             LEFT JOIN worker_profiles worker ON worker.id = draft.drafted_by
             LEFT JOIN jira_comment_deliveries comment ON comment.id = draft.comment_id
             WHERE task.removed_at IS NULL AND ",
            jira_reply_owed!(),
            " ORDER BY draft.drafted_at, task.id LIMIT ?1"
        ))?;
        statement
            .query_map([MAX_JIRA_REPLIES_LISTED], |row| {
                let posted = match (row.get::<_, Option<String>>(8)?, row.get(10)?) {
                    (Some(state), Some(posted_at)) => Some(JiraReplyPosted {
                        state,
                        error: row.get(9)?,
                        posted_at,
                        resolve: row.get(11)?,
                    }),
                    _ => None,
                };
                Ok(JiraReplyAwaiting {
                    task_id: parse_domain_id(&row.get::<_, String>(0)?)?,
                    issue_key: row.get(1)?,
                    project_name: row.get(2)?,
                    title: row.get(3)?,
                    worker_name: row.get(4)?,
                    requester_name: row.get(13)?,
                    body: row.get(5)?,
                    drafted_at: row.get(6)?,
                    resolution_held: row.get(7)?,
                    resolution_waiting: row.get(12)?,
                    posted,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// The operator's press: queues `body`, their edit of the draft, as a
    /// Jira comment and marks the draft posted.
    ///
    /// With `resolve`, the task's held resolution is released when Jira takes
    /// the comment ([`release_resolution_after_reply`]), so the requester is
    /// never sent a resolved ticket without the answer.
    ///
    /// A draft never posted can be posted. So can one whose comment Jira
    /// refused, or that Swarm was not sure it sent: the operator saw that on
    /// the card and pressed again, and the failed attempt is replaced so the
    /// task does not keep showing it.
    ///
    /// # Errors
    /// `JiraReplyNotDrafted` when there is no such draft, or its comment is
    /// on its way or already delivered; `InvalidJiraComment`;
    /// `JiraCommentQueueFull`; a persistence error.
    pub fn post_jira_reply(
        &self,
        task_id: TaskId,
        body: &str,
        resolve: bool,
        now: i64,
    ) -> Result<PostedJiraReply, TaskStoreError> {
        let body = valid_body(body)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let Some((issue_key, failed)) = postable(&transaction, task_id)? else {
            return Err(TaskStoreError::JiraReplyNotDrafted);
        };
        // Whether the ticket resolves once Jira has the reply: the held
        // resolution, when the operator asked for it, or a completion already
        // queued and waiting on this reply.
        let resolution_follows = transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM jira_transition_deliveries
                            WHERE task_id = ?1 AND target_task_state = 'completed'
                              AND (state = 'queued' OR (state = 'held' AND ?2)))",
            params![task_id.to_string(), resolve],
            |row| row.get::<_, bool>(0),
        )?;
        let comment_id = queue_comment_in(&transaction, task_id, body)?;
        transaction.execute(
            "UPDATE jira_reply_drafts
             SET body = ?2, comment_id = ?3, posted_at = ?4, resolve = ?5
             WHERE task_id = ?1",
            params![task_id.to_string(), body, comment_id, now, resolve],
        )?;
        if let Some(failed) = failed {
            transaction.execute(
                "DELETE FROM jira_comment_deliveries WHERE id = ?1",
                [failed],
            )?;
        }
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind)
             VALUES (?1, 'details_updated', ?2, 'operator')",
            params![
                task_id.to_string(),
                format!("The operator posted the reply drafted for {issue_key}.")
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(PostedJiraReply {
            comment_id,
            resolution_follows,
        })
    }

    /// The operator chose not to post the draft. Returns false when there is
    /// nothing to drop: no draft, or its comment is on its way or delivered.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn discard_jira_reply(&self, task_id: TaskId) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let Some((issue_key, failed)) = postable(&transaction, task_id)? else {
            return Ok(false);
        };
        transaction.execute(
            "DELETE FROM jira_reply_drafts WHERE task_id = ?1",
            [task_id.to_string()],
        )?;
        if let Some(failed) = failed {
            transaction.execute(
                "DELETE FROM jira_comment_deliveries WHERE id = ?1",
                [failed],
            )?;
        }
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind)
             VALUES (?1, 'details_updated', ?2, 'operator')",
            params![
                task_id.to_string(),
                format!("The operator chose not to post the reply drafted for {issue_key}.")
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(true)
    }
}

/// Jira took comment `comment_id`. When it is a reply the operator posted
/// with "and resolve", releases the task's held resolution to Jira in the
/// same transaction. Returns whether one was released.
pub(crate) fn release_resolution_after_reply(
    transaction: &rusqlite::Transaction<'_>,
    comment_id: &str,
) -> rusqlite::Result<bool> {
    let held = transaction
        .query_row(
            "SELECT draft.task_id, link.issue_key FROM jira_reply_drafts draft
             JOIN jira_issue_links link ON link.task_id = draft.task_id
             JOIN jira_transition_deliveries held
               ON held.task_id = draft.task_id AND held.state = 'held'
             WHERE draft.comment_id = ?1 AND draft.resolve = 1",
            [comment_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((task_id, issue_key)) = held else {
        return Ok(false);
    };
    transaction.execute(
        "UPDATE jira_transition_deliveries
         SET state = 'queued', available_at = unixepoch(), updated_at = unixepoch()
         WHERE task_id = ?1 AND state = 'held'",
        [&task_id],
    )?;
    transaction.execute(
        "INSERT INTO task_activity (task_id, kind, note, actor_kind)
         VALUES (?1, 'details_updated', ?2, 'operator')",
        params![
            task_id,
            format!(
                "Resolving {issue_key} waited for the operator: they posted the reply and asked \
                 to resolve, so Swarm is resolving it in Jira now that Jira has the reply."
            )
        ],
    )?;
    Ok(true)
}

fn valid_body(body: &str) -> Result<&str, TaskStoreError> {
    let body = body.trim();
    if body.is_empty() || body.len() > MAX_COMMENT_BYTES || body.contains('\0') {
        return Err(TaskStoreError::InvalidJiraComment);
    }
    Ok(body)
}

fn issue_key(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> rusqlite::Result<Option<String>> {
    transaction
        .query_row(
            "SELECT issue_key FROM jira_issue_links WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()
}

/// The issue key of a draft the operator may post or drop, and the failed
/// comment a new press replaces. None when there is no draft, or its comment
/// is on its way or delivered: pressing then would post it twice.
fn postable(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> rusqlite::Result<Option<(String, Option<String>)>> {
    transaction
        .query_row(
            "SELECT link.issue_key, draft.posted_at, comment.id, comment.state
             FROM jira_reply_drafts draft
             JOIN tasks task ON task.id = draft.task_id AND task.removed_at IS NULL
             JOIN jira_issue_links link ON link.task_id = draft.task_id
             LEFT JOIN jira_comment_deliveries comment ON comment.id = draft.comment_id
             WHERE draft.task_id = ?1",
            [task_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map(|found| {
            found.and_then(|(issue_key, posted_at, comment, state)| {
                match (posted_at, state.as_deref()) {
                    (None, _) => Some((issue_key, None)),
                    (Some(_), Some("conflict" | "uncertain")) => Some((issue_key, comment)),
                    _ => None,
                }
            })
        })
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ProviderKind, Task, TaskState};

    use crate::TaskStore;
    use crate::TaskStoreError;
    use crate::github_asks::tests::needs_you_kinds;
    use crate::jira::tests::{
        settle_as_the_coordinator, support_snapshot, support_ticket_in_review,
    };

    const DRAFT: &str = "The removal email still carries the Invited by line.";

    /// A support ticket answered and settled, so its resolution is held for
    /// the operator (ADR 0120), and the worker that answered it.
    fn answered_ticket(store: &TaskStore) -> (Task, swarm_domain::WorkerId) {
        let (_, task) = support_ticket_in_review(store, true);
        settle_as_the_coordinator(store, &task);
        let worker = store
            .create_worker(
                "Ledger",
                ProviderKind::ClaudeCode,
                "/workspace/ledger",
                false,
                1,
            )
            .unwrap();
        (task, worker.id)
    }

    fn awaiting(store: &TaskStore) -> Vec<super::JiraReplyAwaiting> {
        store.jira_replies_awaiting().unwrap()
    }

    /// THE OPERATOR'S ASK: a reply they approve reaches the ticket at once,
    /// with no relay through the worker. Nothing is posted by the draft.
    #[test]
    fn a_drafted_reply_waits_for_the_operator_in_place_of_the_held_resolution() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        assert_eq!(store.held_jira_resolutions().unwrap().len(), 1);

        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();

        let replies = awaiting(&store);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].issue_key, "IS-10736");
        assert_eq!(replies[0].project_name, "IT: Support Requests");
        assert_eq!(replies[0].worker_name.as_deref(), Some("Ledger"));
        assert_eq!(replies[0].body, DRAFT);
        assert!(replies[0].resolution_held);
        assert_eq!(replies[0].posted, None);
        assert!(
            store.claim_jira_comments(2_000_000_000).unwrap().is_empty(),
            "a draft posts nothing"
        );
        // One card, not two: the reply stands in for the held resolution, in
        // the list and in the badge alike.
        assert!(store.held_jira_resolutions().unwrap().is_empty());
        assert_eq!(
            needs_you_kinds(&store),
            vec![("jira_resolution", format!("jira-reply:{}", task.id))]
        );
    }

    #[test]
    fn one_press_posts_the_operators_words_and_the_card_waits_for_jira() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();

        let comment = store
            .post_jira_reply(task.id, "  Edited by the operator.  ", false, 1_600)
            .unwrap()
            .comment_id;

        let claimed = store.claim_jira_comments(2_000_000_000).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, comment);
        assert_eq!(claimed[0].body, "Edited by the operator.");
        let replies = awaiting(&store);
        assert_eq!(replies[0].body, "Edited by the operator.");
        assert_eq!(
            replies[0]
                .posted
                .as_ref()
                .map(|posted| posted.state.as_str()),
            Some("dispatching")
        );
        assert!(
            matches!(
                store.post_jira_reply(task.id, DRAFT, false, 1_700),
                Err(TaskStoreError::JiraReplyNotDrafted)
            ),
            "a press while the first is on its way would post it twice"
        );
        assert!(!store.discard_jira_reply(task.id).unwrap());

        assert!(store.complete_jira_comment(&comment, 1_800).unwrap());
        assert!(awaiting(&store).is_empty());
        // The resolution was not part of this press, so its own card is back,
        // now showing that a reply reached the ticket.
        let held = store.held_jira_resolutions().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].last_reply_at, Some(1_800));
    }

    #[test]
    fn a_reply_jira_refused_stays_on_its_card_and_a_new_press_replaces_it() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();
        let refused = store
            .post_jira_reply(task.id, DRAFT, false, 1_600)
            .unwrap()
            .comment_id;
        store.claim_jira_comments(2_000_000_000).unwrap();
        assert!(
            store
                .fail_jira_comment(&refused, 1_700, false, "jira_rejected")
                .unwrap()
        );

        let posted = awaiting(&store)[0].posted.clone().unwrap();
        assert_eq!(posted.state, "conflict");
        assert_eq!(posted.error.as_deref(), Some("jira_rejected"));
        assert!(store.held_jira_resolutions().unwrap().is_empty());

        let again = store
            .post_jira_reply(task.id, "Shorter.", false, 1_800)
            .unwrap()
            .comment_id;
        assert_ne!(again, refused);
        assert_eq!(store.jira_comment_delivery(&refused).unwrap(), None);
        assert_eq!(
            store
                .jira_comment_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("queued"),
            "the task shows the new attempt, not the refused one"
        );
    }

    /// "Post reply and resolve": the ticket resolves when Jira has the reply,
    /// not when the operator pressed. A refused reply resolves nothing.
    #[test]
    fn post_and_resolve_releases_the_resolution_only_once_jira_has_the_reply() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();

        let refused = store
            .post_jira_reply(task.id, DRAFT, true, 1_600)
            .unwrap()
            .comment_id;
        assert!(awaiting(&store)[0].posted.as_ref().unwrap().resolve);
        store.claim_jira_comments(2_000_000_000).unwrap();
        store
            .fail_jira_comment(&refused, 1_700, false, "permission_denied")
            .unwrap();
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("held"),
            "a reply Jira refused must not resolve the ticket"
        );

        // Pressed again, and Jira could not be reached: it retries by itself.
        let retried = store
            .post_jira_reply(task.id, DRAFT, true, 1_800)
            .unwrap()
            .comment_id;
        store.claim_jira_comments(2_000_000_000).unwrap();
        store
            .fail_jira_comment(&retried, 1_810, true, "network_unavailable")
            .unwrap();
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("held")
        );
        let posted = awaiting(&store)[0].posted.clone().unwrap();
        assert_eq!(posted.state, "queued");
        assert_eq!(posted.error.as_deref(), Some("network_unavailable"));

        // The retry lands, with no second press, and the resolution goes with it.
        assert_eq!(store.claim_jira_comments(2_000_000_000).unwrap().len(), 1);
        assert!(store.complete_jira_comment(&retried, 1_900).unwrap());
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("queued")
        );
        assert_eq!(
            store.claim_jira_transitions(2_000_000_000).unwrap().len(),
            1
        );
        assert!(awaiting(&store).is_empty());
        assert!(store.held_jira_resolutions().unwrap().is_empty());
        assert!(needs_you_kinds(&store).is_empty());
    }

    /// The operator's choice: on a project that does not ask, a completion
    /// resolves the ticket at once, and it used to do so while the reply its
    /// requester was waiting for sat unposted. The resolution now waits for it.
    #[test]
    fn a_completion_waits_for_the_reply_on_a_project_that_does_not_ask() {
        let store = TaskStore::in_memory().unwrap();
        let (_, task) = support_ticket_in_review(&store, false);
        let worker = store
            .create_worker(
                "Ledger",
                ProviderKind::ClaudeCode,
                "/workspace/ledger",
                false,
                1,
            )
            .unwrap();
        store
            .draft_jira_reply(worker.id, task.id, DRAFT, 1_500)
            .unwrap();
        settle_as_the_coordinator(&store, &task);
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("queued")
        );
        assert!(
            store
                .claim_jira_transitions(2_000_000_000)
                .unwrap()
                .is_empty(),
            "the ticket must not resolve ahead of the reply"
        );
        let replies = awaiting(&store);
        assert!(replies[0].resolution_waiting);
        assert!(!replies[0].resolution_held);

        // Posted without "and resolve": the completion was never the
        // operator's to hold, so it follows the reply.
        let posted = store.post_jira_reply(task.id, DRAFT, false, 1_600).unwrap();
        assert!(posted.resolution_follows);
        store.claim_jira_comments(2_000_000_000).unwrap();
        assert!(
            store
                .claim_jira_transitions(2_000_000_000)
                .unwrap()
                .is_empty(),
            "not while the reply is on its way"
        );
        store
            .fail_jira_comment(&posted.comment_id, 1_700, false, "permission_denied")
            .unwrap();
        assert!(
            store
                .claim_jira_transitions(2_000_000_000)
                .unwrap()
                .is_empty(),
            "nor once Jira refused it"
        );
        let again = store.post_jira_reply(task.id, DRAFT, false, 1_800).unwrap();
        store.claim_jira_comments(2_000_000_000).unwrap();
        store
            .complete_jira_comment(&again.comment_id, 1_900)
            .unwrap();
        assert_eq!(
            store.claim_jira_transitions(2_000_000_000).unwrap().len(),
            1
        );
    }

    /// The card says who the reply answers: the ticket's reporter, read on
    /// sync. A sync that no longer sees one clears it.
    #[test]
    fn the_card_names_whoever_raised_the_ticket() {
        let store = TaskStore::in_memory().unwrap();
        let (binding, task) = support_ticket_in_review(&store, false);
        let worker = store
            .create_worker(
                "Ledger",
                ProviderKind::ClaudeCode,
                "/workspace/ledger",
                false,
                1,
            )
            .unwrap();
        store
            .draft_jira_reply(worker.id, task.id, DRAFT, 1_500)
            .unwrap();
        assert_eq!(
            awaiting(&store)[0].requester_name.as_deref(),
            Some("Dana Reyes")
        );

        let hidden = crate::JiraIssueSnapshot {
            reporter_name: Some("   "),
            ..support_snapshot("3", "In Progress")
        };
        store.sync_jira_issues(binding.id, &[hidden]).unwrap();
        assert_eq!(awaiting(&store)[0].requester_name, None);
    }

    #[test]
    fn dropping_the_reply_lets_the_waiting_resolution_go() {
        let store = TaskStore::in_memory().unwrap();
        let (_, task) = support_ticket_in_review(&store, false);
        let worker = store
            .create_worker(
                "Ledger",
                ProviderKind::ClaudeCode,
                "/workspace/ledger",
                false,
                1,
            )
            .unwrap();
        store
            .draft_jira_reply(worker.id, task.id, DRAFT, 1_500)
            .unwrap();
        settle_as_the_coordinator(&store, &task);
        assert!(
            store
                .claim_jira_transitions(2_000_000_000)
                .unwrap()
                .is_empty()
        );

        assert!(store.discard_jira_reply(task.id).unwrap());
        assert_eq!(
            store.claim_jira_transitions(2_000_000_000).unwrap().len(),
            1
        );
    }

    /// "Post reply and resolve" says the ticket resolves only when one was
    /// held for it; with nothing held, nothing follows the reply.
    #[test]
    fn a_resolve_with_nothing_held_does_not_claim_a_resolution() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();
        assert!(store.leave_jira_ticket_open(task.id).unwrap());

        let posted = store.post_jira_reply(task.id, DRAFT, true, 1_600).unwrap();
        assert!(!posted.resolution_follows);
        store.claim_jira_comments(2_000_000_000).unwrap();
        store
            .complete_jira_comment(&posted.comment_id, 1_700)
            .unwrap();
        assert!(
            store
                .claim_jira_transitions(2_000_000_000)
                .unwrap()
                .is_empty()
        );
    }

    /// A new draft never replaces a press on its way, and drops one that
    /// failed so the task's retry cannot send the old words as well.
    #[test]
    fn a_redraft_waits_for_a_press_on_its_way_and_drops_a_failed_one() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();
        let pressed = store.post_jira_reply(task.id, DRAFT, true, 1_600).unwrap();

        assert!(matches!(
            store.draft_jira_reply(worker, task.id, "Second thoughts.", 1_610),
            Err(TaskStoreError::JiraReplyPosting)
        ));
        assert!(awaiting(&store)[0].posted.as_ref().unwrap().resolve);

        store.claim_jira_comments(2_000_000_000).unwrap();
        store
            .fail_jira_comment(&pressed.comment_id, 1_700, false, "permission_denied")
            .unwrap();
        store
            .draft_jira_reply(worker, task.id, "Second thoughts.", 1_800)
            .unwrap();
        assert_eq!(
            store.jira_comment_delivery(&pressed.comment_id).unwrap(),
            None
        );
        assert!(
            !store.retry_jira_comments(task.id).unwrap(),
            "nothing old to resend"
        );
        let replies = awaiting(&store);
        assert_eq!(replies[0].body, "Second thoughts.");
        assert_eq!(replies[0].posted, None);
    }

    #[test]
    fn a_restart_mid_send_leaves_the_reply_uncertain_on_its_card() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();
        let comment = store
            .post_jira_reply(task.id, DRAFT, false, 1_600)
            .unwrap()
            .comment_id;
        store.claim_jira_comments(2_000_000_000).unwrap();

        assert_eq!(store.recover_inflight_jira_comments().unwrap(), 1);

        let posted = awaiting(&store)[0].posted.clone().unwrap();
        assert_eq!(posted.state, "uncertain");
        assert_eq!(posted.error.as_deref(), Some("delivery_interrupted"));
        // The operator may drop it, having found it on the ticket.
        assert!(store.discard_jira_reply(task.id).unwrap());
        assert_eq!(store.jira_comment_delivery(&comment).unwrap(), None);
        assert!(awaiting(&store).is_empty());
    }

    #[test]
    fn dont_post_drops_the_draft_and_the_resolution_card_returns() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, DRAFT, 1_500)
            .unwrap();

        assert!(store.discard_jira_reply(task.id).unwrap());
        assert!(!store.discard_jira_reply(task.id).unwrap());

        assert!(awaiting(&store).is_empty());
        assert_eq!(store.held_jira_resolutions().unwrap().len(), 1);
        assert!(matches!(
            store.post_jira_reply(task.id, DRAFT, false, 1_600),
            Err(TaskStoreError::JiraReplyNotDrafted)
        ));
    }

    #[test]
    fn a_later_draft_replaces_the_earlier_and_a_delivered_reply_can_be_followed() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        store
            .draft_jira_reply(worker, task.id, "First.", 1_500)
            .unwrap();
        store
            .draft_jira_reply(worker, task.id, "Second.", 1_510)
            .unwrap();
        assert_eq!(awaiting(&store)[0].body, "Second.");

        let comment = store
            .post_jira_reply(task.id, "Second.", false, 1_600)
            .unwrap()
            .comment_id;
        store.claim_jira_comments(2_000_000_000).unwrap();
        store.complete_jira_comment(&comment, 1_700).unwrap();
        store
            .draft_jira_reply(worker, task.id, "A follow-up.", 1_800)
            .unwrap();

        let replies = awaiting(&store);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].body, "A follow-up.");
        assert_eq!(replies[0].posted, None);
    }

    /// The briefing names the ticket, so the worker is told how its answer
    /// reaches whoever raised it. A task from anywhere else names none.
    #[test]
    fn a_jira_tickets_briefing_carries_its_issue_key() {
        use std::collections::HashSet;
        let store = TaskStore::in_memory().unwrap();
        let (binding, _) = support_ticket_in_review(&store, false);
        let ticket = store
            .sync_jira_issues(binding.id, &[support_snapshot("1", "Open")])
            .unwrap()
            .remove(0);
        assert_eq!(ticket.state, TaskState::Ready);
        let plain = store
            .create_task("Not from Jira", "/workspace/ledger")
            .unwrap();
        store.transition_task(plain.id, TaskState::Ready).unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        for (task, name) in [(ticket.id, "Ledger"), (plain.id, "Abacus")] {
            let worker = store
                .create_worker(
                    name,
                    ProviderKind::ClaudeCode,
                    "/workspace/ledger",
                    false,
                    1,
                )
                .unwrap();
            let session = swarm_domain::WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            store.assign_task(task, session).unwrap();
        }

        let briefings = store.claim_task_dispatches(100, &HashSet::new()).unwrap();
        let key_for = |task| {
            briefings
                .iter()
                .find(|briefing| briefing.task_id == task)
                .map(|briefing| briefing.jira_issue_key.clone())
        };
        assert_eq!(key_for(ticket.id), Some(Some("IS-10736".to_owned())));
        assert_eq!(key_for(plain.id), Some(None));
    }

    #[test]
    fn only_a_jira_ticket_takes_a_reply_and_only_one_jira_would_accept() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = answered_ticket(&store);
        let plain = store
            .create_task("Not from Jira", "/workspace/ledger")
            .unwrap();
        assert!(matches!(
            store.draft_jira_reply(worker, plain.id, DRAFT, 1),
            Err(TaskStoreError::NotFound)
        ));
        for body in ["   ", "nul\0here", &"x".repeat(4_001)] {
            assert!(matches!(
                store.draft_jira_reply(worker, task.id, body, 1),
                Err(TaskStoreError::InvalidJiraComment)
            ));
        }
        assert_eq!(
            store.get_task(task.id).unwrap().state,
            TaskState::Completed,
            "drafting moves nothing"
        );
    }
}
