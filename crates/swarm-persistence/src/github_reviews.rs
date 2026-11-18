//! Reviews the operator submits from Needs you (ADR 0176).
//!
//! A worker that read a pull request PROPOSES a review: a verdict, a comment to
//! the author, and the commit it read. It never posts one. The operator reads
//! the proposal on the owed-review card, edits the comment if they want, and
//! presses. Only that press posts, under their GitHub login.
//!
//! The press is recorded here BEFORE GitHub is called, and every step after it
//! is a recorded move. A call whose outcome is unknown is never repeated blind:
//! it is settled by reading GitHub back, as ADR 0173 does for messages.
//!
//! Both tables are scoped to the ask's ROUND. A review requested again after
//! the operator reviewed reuses the same link, and the earlier round's review
//! must not answer the new request.

use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{TaskId, WorkerId};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

/// The longest comment a proposal or a submission carries. GitHub takes far
/// more; this bounds what a card shows and what one press posts.
pub const MAX_GITHUB_REVIEW_COMMENT_CHARS: usize = 6_000;
/// Submissions kept per ask. Older ones are history GitHub already holds, and
/// a run of refused presses must not grow the table without end.
const SUBMISSIONS_KEPT_PER_ASK: i64 = 10;
/// The longest outcome kept, in the operator's words or GitHub's.
const MAX_OUTCOME_BYTES: usize = 500;
/// The most unsettled submissions one settling pass reads.
const MAX_UNSETTLED_LISTED: i64 = 20;

/// Adds the proposal and submission tables, and the round each ask is on
/// (schema 230).
pub(crate) fn migrate_github_review_submissions(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    let has_round: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('github_ask_links') WHERE name = 'round')",
        [],
        |row| row.get(0),
    )?;
    if !has_round {
        transaction.execute_batch(
            "ALTER TABLE github_ask_links ADD COLUMN round INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_review_proposals (
             url TEXT PRIMARY KEY REFERENCES github_ask_links(url) ON DELETE CASCADE,
             round INTEGER NOT NULL CHECK (round >= 1),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             proposed_by TEXT NOT NULL,
             verdict TEXT NOT NULL CHECK (verdict IN ('approve','request_changes')),
             comment TEXT NOT NULL CHECK (length(comment) <= 6000),
             head_sha TEXT NOT NULL CHECK (length(head_sha) = 40),
             proposed_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS github_review_submissions (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             url TEXT NOT NULL REFERENCES github_ask_links(url) ON DELETE CASCADE,
             round INTEGER NOT NULL CHECK (round >= 1),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             verdict TEXT NOT NULL CHECK (verdict IN ('approve','request_changes')),
             comment TEXT NOT NULL CHECK (length(comment) <= 6000),
             head_sha TEXT NOT NULL CHECK (length(head_sha) = 40),
             state TEXT NOT NULL CHECK (state IN (
                 'queued','posting','post_uncertain','failed',
                 'awaiting_merge','merging','merge_uncertain','merged','done'
             )),
             outcome TEXT,
             review_id INTEGER,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS github_review_submissions_one_in_flight
             ON github_review_submissions(url)
             WHERE state IN ('queued','posting','post_uncertain','awaiting_merge',
                             'merging','merge_uncertain');",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// What the worker suggests the operator does with the pull request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubReviewVerdict {
    Approve,
    RequestChanges,
}

impl GithubReviewVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::RequestChanges => "request_changes",
        }
    }

    /// The verdict a stored or sent value names, or `None` for anything else.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "approve" => Some(Self::Approve),
            "request_changes" => Some(Self::RequestChanges),
            _ => None,
        }
    }
}

/// The review a worker proposed for the operator to submit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GithubReviewProposal {
    pub verdict: GithubReviewVerdict,
    /// Written to the pull request's author. Posted under the operator's name
    /// only when they press.
    pub comment: String,
    /// The full commit the worker read.
    pub head_sha: String,
    pub proposed_at: i64,
}

/// Why a worker's proposal was not recorded, in words the worker can act on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GithubReviewProposalRefusal {
    /// The task is not this worker's, or not a GitHub review request.
    NotYourReview,
    /// GitHub no longer asks for this review.
    NoLongerAsked,
    /// Not a full 40-character commit.
    HeadNotACommit,
    /// Requesting changes says nothing without a comment.
    CommentRequired,
    TooLong,
}

impl GithubReviewProposalRefusal {
    #[must_use]
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::NotYourReview => "this task is not a GitHub review request assigned to you",
            Self::NoLongerAsked => {
                "GitHub no longer asks for this review, so there is nothing to submit"
            }
            Self::HeadNotACommit => {
                "head_sha must be the full 40-character commit you read, for example from \
                 `gh pr view <number> --json headRefOid`"
            }
            Self::CommentRequired => "requesting changes needs a comment saying what to change",
            Self::TooLong => "the comment is longer than 6000 characters",
        }
    }
}

/// Where one press stands. The moves that exist are [`Self::can_move_to`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubReviewSubmissionState {
    /// Pressed; GitHub not called yet.
    Queued,
    /// The review is being posted.
    Posting,
    /// The post's outcome is unknown. Settled by reading GitHub back, never by
    /// posting again.
    PostUncertain,
    /// GitHub refused the review, or it was never sent. The operator may press
    /// again.
    Failed,
    /// The approval is on GitHub and a merge is still to be tried.
    AwaitingMerge,
    Merging,
    /// The merge's outcome is unknown. Settled by reading the pull request.
    MergeUncertain,
    Merged,
    /// The review is on GitHub and nothing more will happen: changes were
    /// requested, or the merge did not happen and the outcome says why.
    Done,
}

impl GithubReviewSubmissionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Posting => "posting",
            Self::PostUncertain => "post_uncertain",
            Self::Failed => "failed",
            Self::AwaitingMerge => "awaiting_merge",
            Self::Merging => "merging",
            Self::MergeUncertain => "merge_uncertain",
            Self::Merged => "merged",
            Self::Done => "done",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Posting,
            Self::PostUncertain,
            Self::Failed,
            Self::AwaitingMerge,
            Self::Merging,
            Self::MergeUncertain,
            Self::Merged,
            Self::Done,
        ]
        .into_iter()
        .find(|state| state.as_str() == value)
    }

    /// Every move a submission can make. Anything else is refused.
    #[must_use]
    pub const fn can_move_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Posting)
                | (
                    Self::Posting,
                    Self::AwaitingMerge | Self::Done | Self::Failed | Self::PostUncertain
                )
                | (
                    Self::PostUncertain,
                    Self::AwaitingMerge | Self::Done | Self::Failed
                )
                | (Self::AwaitingMerge, Self::Merging | Self::Done)
                | (
                    Self::Merging,
                    Self::Merged | Self::Done | Self::MergeUncertain
                )
                | (Self::MergeUncertain, Self::Merged | Self::Done)
        )
    }

    /// Whether the review is on GitHub, so the operator no longer owes it.
    #[must_use]
    pub const fn review_is_posted(self) -> bool {
        matches!(
            self,
            Self::AwaitingMerge | Self::Merging | Self::MergeUncertain | Self::Merged | Self::Done
        )
    }
}

/// The latest press on an owed review, as its card shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GithubReviewSubmissionView {
    pub id: i64,
    pub state: GithubReviewSubmissionState,
    /// Why it failed, why it did not merge, or what is being checked.
    pub outcome: Option<String>,
}

/// One submission, with what GitHub needs to carry it out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubReviewDispatch {
    pub id: i64,
    pub task_id: TaskId,
    pub repository: String,
    pub number: u64,
    pub verdict: GithubReviewVerdict,
    pub comment: String,
    pub head_sha: String,
    pub state: GithubReviewSubmissionState,
    /// Why it failed, why it did not merge, or what is being checked.
    pub outcome: Option<String>,
    pub review_id: Option<i64>,
    pub created_at: i64,
}

/// Why a press was not recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GithubReviewSubmissionRefusal {
    /// No review is owed on this task any more.
    NotOwed,
    /// The worker recorded no review to submit.
    NoProposal,
    /// An earlier press is still being carried out.
    InFlight,
    CommentRequired,
    TooLong,
}

impl GithubReviewSubmissionRefusal {
    #[must_use]
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::NotOwed => "This review is no longer waiting for you",
            Self::NoProposal => "The worker recorded no review to submit",
            Self::InFlight => "This review is already being submitted",
            Self::CommentRequired => "Requesting changes needs a comment",
            Self::TooLong => "The comment is longer than 6000 characters",
        }
    }
}

fn is_full_commit(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn comment_refusal(
    verdict: GithubReviewVerdict,
    comment: &str,
) -> Option<GithubReviewSubmissionRefusal> {
    if comment.chars().count() > MAX_GITHUB_REVIEW_COMMENT_CHARS {
        Some(GithubReviewSubmissionRefusal::TooLong)
    } else if verdict == GithubReviewVerdict::RequestChanges && comment.trim().is_empty() {
        Some(GithubReviewSubmissionRefusal::CommentRequired)
    } else {
        None
    }
}

fn clamp_outcome(text: &str) -> String {
    let mut end = text.len().min(MAX_OUTCOME_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

impl TaskStore {
    /// Records the review a worker proposes for the operator to submit,
    /// replacing any earlier one for the same request (ADR 0176).
    ///
    /// Only the worker the review task is assigned to may propose, and only
    /// while GitHub still asks. Nothing is posted: the operator's press does
    /// that.
    ///
    /// # Errors
    /// The outer error is persistence; the inner one is a refusal to show the
    /// worker.
    pub fn propose_github_review(
        &self,
        worker: WorkerId,
        task: TaskId,
        verdict: GithubReviewVerdict,
        comment: &str,
        head_sha: &str,
        now: i64,
    ) -> Result<Result<(), GithubReviewProposalRefusal>, TaskStoreError> {
        if !is_full_commit(head_sha) {
            return Ok(Err(GithubReviewProposalRefusal::HeadNotACommit));
        }
        match comment_refusal(verdict, comment) {
            Some(GithubReviewSubmissionRefusal::TooLong) => {
                return Ok(Err(GithubReviewProposalRefusal::TooLong));
            }
            Some(_) => return Ok(Err(GithubReviewProposalRefusal::CommentRequired)),
            None => {}
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let link: Option<(String, i64, Option<i64>)> = transaction
            .query_row(
                "SELECT link.url, link.round, link.ended_at
                 FROM github_ask_links link JOIN tasks task ON task.id = link.task_id
                 WHERE link.task_id = ?1 AND link.kind = 'review_request'
                   AND task.assigned_worker_id = ?2 AND task.removed_at IS NULL",
                params![task.to_string(), worker.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((url, round, ended_at)) = link else {
            return Ok(Err(GithubReviewProposalRefusal::NotYourReview));
        };
        if ended_at.is_some() {
            return Ok(Err(GithubReviewProposalRefusal::NoLongerAsked));
        }
        transaction.execute(
            "INSERT INTO github_review_proposals
                 (url, round, task_id, proposed_by, verdict, comment, head_sha, proposed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(url) DO UPDATE SET
                 round = excluded.round, task_id = excluded.task_id,
                 proposed_by = excluded.proposed_by, verdict = excluded.verdict,
                 comment = excluded.comment, head_sha = excluded.head_sha,
                 proposed_at = excluded.proposed_at",
            params![
                url,
                round,
                task.to_string(),
                worker.to_string(),
                verdict.as_str(),
                comment,
                head_sha.to_ascii_lowercase(),
                now,
            ],
        )?;
        crate::events::insert_control_room_event(
            &transaction,
            crate::ControlRoomEventKind::TasksChanged,
        )?;
        transaction.commit()?;
        Ok(Ok(()))
    }

    /// The proposal for the request a task's link is on now, if its worker
    /// recorded one this round.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a row is corrupt.
    pub fn github_review_proposal(
        &self,
        task: TaskId,
    ) -> Result<Option<GithubReviewProposal>, TaskStoreError> {
        let connection = self.connection()?;
        proposal_for(&connection, task)
    }

    /// Records the operator's press on an owed review (ADR 0176), before
    /// GitHub is called.
    ///
    /// The commit is the one the worker read, never whatever the pull request
    /// holds now: the operator approves what was reviewed.
    ///
    /// # Errors
    /// The outer error is persistence; the inner one is a refusal to show the
    /// operator.
    pub fn queue_github_review_submission(
        &self,
        task: TaskId,
        verdict: GithubReviewVerdict,
        comment: &str,
        now: i64,
    ) -> Result<Result<i64, GithubReviewSubmissionRefusal>, TaskStoreError> {
        if let Some(refusal) = comment_refusal(verdict, comment) {
            return Ok(Err(refusal));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let owed: Option<(String, i64)> = transaction
            .query_row(
                concat!(
                    "SELECT link.url, link.round FROM github_ask_links link
                     JOIN tasks task ON task.id = link.task_id
                     WHERE link.task_id = ?1 AND ",
                    crate::github_asks::github_review_owed!()
                ),
                [task.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((url, round)) = owed else {
            return Ok(Err(GithubReviewSubmissionRefusal::NotOwed));
        };
        let Some(proposal) = proposal_for(&transaction, task)? else {
            return Ok(Err(GithubReviewSubmissionRefusal::NoProposal));
        };
        let in_flight: bool = transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM github_review_submissions
                 WHERE url = ?1 AND state IN ('queued','posting','post_uncertain',
                     'awaiting_merge','merging','merge_uncertain'))",
            [&url],
            |row| row.get(0),
        )?;
        if in_flight {
            return Ok(Err(GithubReviewSubmissionRefusal::InFlight));
        }
        transaction.execute(
            "INSERT INTO github_review_submissions
                 (url, round, task_id, verdict, comment, head_sha, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?7)",
            params![
                url,
                round,
                task.to_string(),
                verdict.as_str(),
                comment,
                proposal.head_sha,
                now,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        transaction.execute(
            "DELETE FROM github_review_submissions
             WHERE url = ?1 AND id NOT IN (
                 SELECT id FROM github_review_submissions WHERE url = ?1
                 ORDER BY id DESC LIMIT ?2)",
            params![url, SUBMISSIONS_KEPT_PER_ASK],
        )?;
        crate::events::insert_control_room_event(
            &transaction,
            crate::ControlRoomEventKind::TasksChanged,
        )?;
        transaction.commit()?;
        Ok(Ok(id))
    }

    /// One submission, with what GitHub needs to carry it out.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a row is corrupt.
    pub fn github_review_submission(
        &self,
        id: i64,
    ) -> Result<Option<GithubReviewDispatch>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!("{DISPATCH_COLUMNS} WHERE submission.id = ?1"),
                [id],
                dispatch_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Submissions a press left unfinished: queued and never sent, or a
    /// merge still to try, or an outcome GitHub has to be asked about.
    /// Oldest first, bounded.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a row is corrupt.
    pub fn unsettled_github_review_submissions(
        &self,
    ) -> Result<Vec<GithubReviewDispatch>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "{DISPATCH_COLUMNS} WHERE submission.state IN
                 ('queued','post_uncertain','awaiting_merge','merge_uncertain')
             ORDER BY submission.id LIMIT ?1"
        ))?;
        statement
            .query_map([MAX_UNSETTLED_LISTED], dispatch_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Moves a submission one step, if that move exists from where it is now.
    ///
    /// Returns false when the submission is not where the caller expected,
    /// so two settlers racing on one press cannot both act on it.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a row is corrupt.
    pub fn move_github_review_submission(
        &self,
        id: i64,
        from: GithubReviewSubmissionState,
        to: GithubReviewSubmissionState,
        outcome: Option<&str>,
        review_id: Option<i64>,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        if !from.can_move_to(to) {
            return Ok(false);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let moved = transaction.execute(
            "UPDATE github_review_submissions
             SET state = ?3, outcome = COALESCE(?4, outcome),
                 review_id = COALESCE(?5, review_id), updated_at = ?6
             WHERE id = ?1 AND state = ?2",
            params![
                id,
                from.as_str(),
                to.as_str(),
                outcome.map(clamp_outcome),
                review_id,
                now,
            ],
        )?;
        if moved == 1 {
            crate::events::insert_control_room_event(
                &transaction,
                crate::ControlRoomEventKind::TasksChanged,
            )?;
        }
        transaction.commit()?;
        Ok(moved == 1)
    }

    /// After a restart, a call that was in flight has an unknown outcome:
    /// GitHub may have taken it. Each becomes uncertain, to be settled by
    /// reading GitHub back rather than by sending again.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn recover_inflight_github_review_submissions(
        &self,
        now: i64,
    ) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        let posting = connection.execute(
            "UPDATE github_review_submissions
             SET state = 'post_uncertain', updated_at = ?1,
                 outcome = 'Swarm restarted while posting this review'
             WHERE state = 'posting'",
            [now],
        )?;
        let merging = connection.execute(
            "UPDATE github_review_submissions
             SET state = 'merge_uncertain', updated_at = ?1,
                 outcome = 'Swarm restarted while merging this pull request'
             WHERE state = 'merging'",
            [now],
        )?;
        Ok(posting + merging)
    }
}

const DISPATCH_COLUMNS: &str = "SELECT submission.id, submission.task_id, link.repository,
        link.number, submission.verdict, submission.comment, submission.head_sha,
        submission.state, submission.review_id, submission.created_at, submission.outcome
     FROM github_review_submissions submission
     JOIN github_ask_links link ON link.url = submission.url";

fn dispatch_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GithubReviewDispatch> {
    let corrupt = |column: usize| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            "an unknown value in a GitHub review submission".into(),
        )
    };
    let verdict: String = row.get(4)?;
    let state: String = row.get(7)?;
    let number: i64 = row.get(3)?;
    Ok(GithubReviewDispatch {
        id: row.get(0)?,
        task_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        repository: row.get(2)?,
        number: u64::try_from(number).map_err(|_| corrupt(3))?,
        verdict: GithubReviewVerdict::parse(&verdict).ok_or_else(|| corrupt(4))?,
        comment: row.get(5)?,
        head_sha: row.get(6)?,
        state: GithubReviewSubmissionState::parse(&state).ok_or_else(|| corrupt(7))?,
        outcome: row.get(10)?,
        review_id: row.get(8)?,
        created_at: row.get(9)?,
    })
}

/// The proposal on the request a task's link carries now.
pub(crate) fn proposal_for(
    connection: &rusqlite::Connection,
    task: TaskId,
) -> Result<Option<GithubReviewProposal>, TaskStoreError> {
    let row: Option<(String, String, String, i64)> = connection
        .query_row(
            "SELECT proposal.verdict, proposal.comment, proposal.head_sha, proposal.proposed_at
             FROM github_ask_links link
             JOIN github_review_proposals proposal
               ON proposal.url = link.url AND proposal.round = link.round
                  AND proposal.task_id = link.task_id
             WHERE link.task_id = ?1",
            [task.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((verdict, comment, head_sha, proposed_at)) = row else {
        return Ok(None);
    };
    let verdict = GithubReviewVerdict::parse(&verdict).ok_or_else(|| {
        TaskStoreError::from(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            "an unknown review verdict".into(),
        ))
    })?;
    Ok(Some(GithubReviewProposal {
        verdict,
        comment,
        head_sha,
        proposed_at,
    }))
}

/// The latest press this round, while the review is still owed: queued,
/// being posted, uncertain, or refused. A posted review is no longer owed,
/// so its card is gone.
pub(crate) fn latest_unposted_submission(
    connection: &rusqlite::Connection,
    task: TaskId,
) -> Result<Option<GithubReviewSubmissionView>, TaskStoreError> {
    let row: Option<(i64, String, Option<String>)> = connection
        .query_row(
            "SELECT submission.id, submission.state, submission.outcome
             FROM github_ask_links link
             JOIN github_review_submissions submission
               ON submission.url = link.url AND submission.round = link.round
                  AND submission.task_id = link.task_id
             WHERE link.task_id = ?1
             ORDER BY submission.id DESC LIMIT 1",
            [task.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((id, state, outcome)) = row else {
        return Ok(None);
    };
    let state = GithubReviewSubmissionState::parse(&state).ok_or_else(|| {
        TaskStoreError::from(rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            "an unknown submission state".into(),
        ))
    })?;
    Ok((!state.review_is_posted()).then_some(GithubReviewSubmissionView { id, state, outcome }))
}

#[cfg(test)]
mod tests {
    use swarm_domain::{AskEnded, ProviderKind, TaskState};

    use super::{
        GithubReviewProposalRefusal, GithubReviewSubmissionRefusal, GithubReviewSubmissionState,
        GithubReviewVerdict,
    };
    use crate::TaskStore;
    use crate::github_asks::tests::{
        REVIEW_URL, needs_you_kinds, notes_written, owed_tasks, receive, review, routed_review,
    };

    const HEAD: &str = "888059e91c0ffee0123456789abcdef012345678";
    const COMMENT: &str = "Has a full sync been run with the read-only role?";

    use GithubReviewSubmissionState as S;
    const ALL_STATES: [S; 9] = [
        S::Queued,
        S::Posting,
        S::PostUncertain,
        S::Failed,
        S::AwaitingMerge,
        S::Merging,
        S::MergeUncertain,
        S::Merged,
        S::Done,
    ];

    /// A review whose notes are written, with the worker's proposal on it.
    fn proposed(store: &TaskStore) -> swarm_domain::TaskId {
        let (task, worker) = routed_review(store);
        notes_written(store, task);
        store
            .propose_github_review(
                worker,
                task,
                GithubReviewVerdict::Approve,
                COMMENT,
                HEAD,
                1_100,
            )
            .unwrap()
            .unwrap();
        task
    }

    fn press(store: &TaskStore, task: swarm_domain::TaskId) -> i64 {
        store
            .queue_github_review_submission(task, GithubReviewVerdict::Approve, COMMENT, 1_200)
            .unwrap()
            .unwrap()
    }

    /// Walks a submission along existing moves to `target`.
    fn walk_to(store: &TaskStore, id: i64, target: S) {
        let path: &[S] = match target {
            S::Queued => &[],
            S::Posting => &[S::Posting],
            S::PostUncertain => &[S::Posting, S::PostUncertain],
            S::Failed => &[S::Posting, S::Failed],
            S::AwaitingMerge => &[S::Posting, S::AwaitingMerge],
            S::Merging => &[S::Posting, S::AwaitingMerge, S::Merging],
            S::MergeUncertain => &[S::Posting, S::AwaitingMerge, S::Merging, S::MergeUncertain],
            S::Merged => &[S::Posting, S::AwaitingMerge, S::Merging, S::Merged],
            S::Done => &[S::Posting, S::Done],
        };
        let mut at = S::Queued;
        for &next in path {
            assert!(
                store
                    .move_github_review_submission(id, at, next, None, None, 1_300)
                    .unwrap(),
                "{at:?} -> {next:?}"
            );
            at = next;
        }
    }

    /// THE CARD AND THE BADGE READ ONE PREDICATE, and that predicate names the
    /// posted states in SQL while the enum names them in Rust. This holds the
    /// two together: a state added to one and not the other fails here.
    #[test]
    fn a_review_leaves_needs_you_exactly_when_it_is_posted() {
        for state in ALL_STATES {
            let store = TaskStore::in_memory().unwrap();
            let task = proposed(&store);
            let id = press(&store, task);
            walk_to(&store, id, state);
            let owed = !owed_tasks(&store).is_empty();
            assert_eq!(owed, !state.review_is_posted(), "{state:?}");
            assert_eq!(
                needs_you_kinds(&store)
                    .iter()
                    .any(|(kind, _)| *kind == "github_review"),
                owed,
                "the badge must agree with the card in {state:?}"
            );
        }
    }

    #[test]
    fn the_moves_that_exist_and_no_others() {
        let allowed = [
            (S::Queued, S::Posting),
            (S::Posting, S::AwaitingMerge),
            (S::Posting, S::Done),
            (S::Posting, S::Failed),
            (S::Posting, S::PostUncertain),
            (S::PostUncertain, S::AwaitingMerge),
            (S::PostUncertain, S::Done),
            (S::PostUncertain, S::Failed),
            (S::AwaitingMerge, S::Merging),
            (S::AwaitingMerge, S::Done),
            (S::Merging, S::Merged),
            (S::Merging, S::Done),
            (S::Merging, S::MergeUncertain),
            (S::MergeUncertain, S::Merged),
            (S::MergeUncertain, S::Done),
        ];
        for from in ALL_STATES {
            for to in ALL_STATES {
                assert_eq!(
                    from.can_move_to(to),
                    allowed.contains(&(from, to)),
                    "{from:?} -> {to:?}"
                );
            }
        }
        // A move from somewhere the submission is not changes nothing, so two
        // settlers racing on one press cannot both act.
        let store = TaskStore::in_memory().unwrap();
        let task = proposed(&store);
        let id = press(&store, task);
        assert!(
            !store
                .move_github_review_submission(id, S::Posting, S::Failed, None, None, 1_300)
                .unwrap()
        );
        assert!(
            !store
                .move_github_review_submission(id, S::Queued, S::Merged, None, None, 1_300)
                .unwrap()
        );
        assert_eq!(
            store.github_review_submission(id).unwrap().unwrap().state,
            S::Queued
        );
    }

    #[test]
    fn only_the_assigned_worker_proposes_and_only_while_github_asks() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = routed_review(&store);
        let stranger = store
            .create_worker(
                "Moss",
                ProviderKind::ClaudeCode,
                "/workspace/moss",
                false,
                2,
            )
            .unwrap();
        let propose = |who, verdict, comment: &str, head: &str| {
            store
                .propose_github_review(who, task, verdict, comment, head, 1_100)
                .unwrap()
        };
        assert_eq!(
            propose(stranger.id, GithubReviewVerdict::Approve, COMMENT, HEAD),
            Err(GithubReviewProposalRefusal::NotYourReview)
        );
        assert_eq!(
            propose(worker, GithubReviewVerdict::Approve, COMMENT, "888059e9"),
            Err(GithubReviewProposalRefusal::HeadNotACommit),
            "a short commit cannot pin a review or guard a merge"
        );
        assert_eq!(
            propose(worker, GithubReviewVerdict::RequestChanges, "  ", HEAD),
            Err(GithubReviewProposalRefusal::CommentRequired)
        );
        assert_eq!(
            propose(
                worker,
                GithubReviewVerdict::Approve,
                &"x".repeat(6_001),
                HEAD
            ),
            Err(GithubReviewProposalRefusal::TooLong)
        );
        assert_eq!(
            propose(worker, GithubReviewVerdict::Approve, "", HEAD),
            Ok(())
        );
        assert_eq!(
            propose(
                worker,
                GithubReviewVerdict::RequestChanges,
                COMMENT,
                &HEAD.to_uppercase()
            ),
            Ok(())
        );
        let proposal = store.github_review_proposal(task).unwrap().unwrap();
        assert_eq!(
            proposal.verdict,
            GithubReviewVerdict::RequestChanges,
            "the later one replaces"
        );
        assert_eq!(proposal.head_sha, HEAD, "stored as GitHub writes it");

        store
            .record_github_ask_ended(REVIEW_URL, AskEnded::ReviewNoLongerRequested)
            .unwrap();
        assert_eq!(
            propose(worker, GithubReviewVerdict::Approve, COMMENT, HEAD),
            Err(GithubReviewProposalRefusal::NoLongerAsked)
        );
    }

    #[test]
    fn the_proposal_reaches_the_card_and_a_new_request_starts_without_it() {
        let store = TaskStore::in_memory().unwrap();
        let task = proposed(&store);
        let owed = store.github_reviews_owed().unwrap();
        let proposal = owed[0].proposal.as_ref().unwrap();
        assert_eq!(
            (
                proposal.verdict,
                proposal.comment.as_str(),
                proposal.head_sha.as_str()
            ),
            (GithubReviewVerdict::Approve, COMMENT, HEAD)
        );

        // The operator submits; GitHub stops asking, then asks again about the
        // same pull request while the task is still open.
        let id = press(&store, task);
        walk_to(&store, id, S::Done);
        store.transition_task(task, TaskState::Active).unwrap();
        store
            .record_github_ask_ended(REVIEW_URL, AskEnded::ReviewNoLongerRequested)
            .unwrap();
        receive(&store, &review(), None);
        notes_written(&store, task);
        let owed = store.github_reviews_owed().unwrap();
        assert_eq!(
            owed.len(),
            1,
            "last round's review does not answer this request"
        );
        assert_eq!(owed[0].proposal, None, "nor does last round's proposal");
        assert_eq!(owed[0].submission, None);
    }

    #[test]
    fn a_press_is_recorded_before_github_and_one_at_a_time() {
        let store = TaskStore::in_memory().unwrap();
        let (task, worker) = routed_review(&store);
        store.transition_task(task, TaskState::Active).unwrap();
        let queue = |verdict, comment: &str| {
            store
                .queue_github_review_submission(task, verdict, comment, 1_200)
                .unwrap()
        };
        assert_eq!(
            queue(GithubReviewVerdict::Approve, COMMENT),
            Err(GithubReviewSubmissionRefusal::NotOwed),
            "work still being done is not the operator's to submit"
        );
        notes_written(&store, task);
        assert_eq!(
            queue(GithubReviewVerdict::Approve, COMMENT),
            Err(GithubReviewSubmissionRefusal::NoProposal)
        );
        store
            .propose_github_review(
                worker,
                task,
                GithubReviewVerdict::Approve,
                COMMENT,
                HEAD,
                1_100,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            queue(GithubReviewVerdict::RequestChanges, ""),
            Err(GithubReviewSubmissionRefusal::CommentRequired)
        );
        let edited = "Approved. One question: has a full sync run with the role?";
        let id = queue(GithubReviewVerdict::Approve, edited).unwrap();
        assert_eq!(
            queue(GithubReviewVerdict::Approve, edited),
            Err(GithubReviewSubmissionRefusal::InFlight),
            "a second press while the first is carried out would post twice"
        );
        let dispatch = store.github_review_submission(id).unwrap().unwrap();
        assert_eq!(dispatch.state, S::Queued);
        assert_eq!(
            dispatch.comment, edited,
            "the operator's words, not the proposal's"
        );
        assert_eq!(
            dispatch.head_sha, HEAD,
            "pinned to the commit the worker read"
        );
        assert_eq!(
            (dispatch.repository.as_str(), dispatch.number),
            ("acme/petal", 42)
        );
        assert_eq!(dispatch.task_id, task);
    }

    #[test]
    fn a_refused_review_stays_on_the_card_with_githubs_words_and_can_be_pressed_again() {
        let store = TaskStore::in_memory().unwrap();
        let task = proposed(&store);
        let id = press(&store, task);
        store
            .move_github_review_submission(id, S::Queued, S::Posting, None, None, 1_300)
            .unwrap();
        store
            .move_github_review_submission(
                id,
                S::Posting,
                S::Failed,
                Some("Can not approve your own pull request"),
                None,
                1_300,
            )
            .unwrap();
        let owed = store.github_reviews_owed().unwrap();
        let shown = owed[0].submission.as_ref().unwrap();
        assert_eq!(shown.state, S::Failed);
        assert_eq!(
            shown.outcome.as_deref(),
            Some("Can not approve your own pull request")
        );
        assert!(press(&store, task) > id);
    }

    #[test]
    fn a_restart_leaves_an_unsent_call_uncertain_never_sent_again() {
        let store = TaskStore::in_memory().unwrap();
        let mut ids = Vec::new();
        for state in [
            S::Posting,
            S::Merging,
            S::Queued,
            S::AwaitingMerge,
            S::Failed,
            S::Done,
        ] {
            let task = {
                // One ask per state: a fresh review on its own pull request.
                let (task, worker) = routed_review_numbered(&store, ids.len());
                notes_written(&store, task);
                store
                    .propose_github_review(
                        worker,
                        task,
                        GithubReviewVerdict::Approve,
                        "",
                        HEAD,
                        1_100,
                    )
                    .unwrap()
                    .unwrap();
                task
            };
            let id = press(&store, task);
            walk_to(&store, id, state);
            ids.push((id, state));
        }
        assert_eq!(
            store
                .recover_inflight_github_review_submissions(1_400)
                .unwrap(),
            2
        );
        let state_of = |id| store.github_review_submission(id).unwrap().unwrap().state;
        assert_eq!(state_of(ids[0].0), S::PostUncertain);
        assert_eq!(state_of(ids[1].0), S::MergeUncertain);
        let unsettled = store
            .unsettled_github_review_submissions()
            .unwrap()
            .into_iter()
            .map(|dispatch| dispatch.state)
            .collect::<Vec<_>>();
        assert_eq!(
            unsettled,
            vec![
                S::PostUncertain,
                S::MergeUncertain,
                S::Queued,
                S::AwaitingMerge
            ],
            "failed and done are finished; the rest each still need GitHub"
        );
    }

    #[test]
    fn refused_presses_do_not_grow_without_end() {
        let store = TaskStore::in_memory().unwrap();
        let task = proposed(&store);
        for _ in 0..15 {
            let id = press(&store, task);
            walk_to(&store, id, S::Failed);
        }
        let connection = store.connection().unwrap();
        let kept: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM github_review_submissions",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, 10);
    }

    /// A routed review on pull request 100 + n, assigned to its own worker.
    fn routed_review_numbered(
        store: &TaskStore,
        n: usize,
    ) -> (swarm_domain::TaskId, swarm_domain::WorkerId) {
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                &format!("Petal {n}"),
                ProviderKind::ClaudeCode,
                &format!("/workspace/petal-{n}"),
                false,
                i64::try_from(n).unwrap() + 10,
            )
            .unwrap();
        let ask = swarm_domain::GithubAsk::new(
            swarm_domain::GithubAskKind::ReviewRequest,
            "acme/petal",
            100 + u64::try_from(n).unwrap(),
            "Make the petals bloom",
            Some("sorrel"),
            "",
        )
        .unwrap();
        let task = match receive(store, &ask, Some(worker.id)) {
            crate::GithubAskArrival::Filed(task) => task,
            other => panic!("expected a filing, got {other:?}"),
        };
        store.transition_task(task, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task, worker.id).unwrap();
        (task, worker.id)
    }
}
