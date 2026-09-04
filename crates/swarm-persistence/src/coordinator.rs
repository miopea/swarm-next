use rusqlite::params;
use std::collections::HashMap;
use std::str::FromStr;
use swarm_domain::{ControlRoomEventKind, TaskId, WorkerId, WorkerSessionId};
use uuid::Uuid;

use super::{
    TaskStore, TaskStoreError, WORKER_FILED_DRAFT_SCHEMA_VERSION, events::insert_control_room_event,
};

/// Automatic starts are intentionally serialized. A fresh resource sample is
/// required before the next sleeping worker can be claimed.
pub const AUTOMATIC_WAKE_BATCH_LIMIT: u8 = 1;
const MAX_WAKE_CLAIMS: i64 = AUTOMATIC_WAKE_BATCH_LIMIT as i64;
const MAX_STALE_CANDIDATES: i64 = 32;
const MAX_EXITED_WORK_CANDIDATES: i64 = 32;
const MAX_UNSTARTED_WORK_CANDIDATES: i64 = 32;

/// The last moment a worker or the operator did something to a task itself.
///
/// One definition, because two flags need it and a second copy is how they
/// drift. Joined as `acted`, exposing `task_id` and `acted_at`; LEFT JOIN it,
/// because a task nobody has touched yields no row rather than a zero.
///
/// ONE SOURCE, and it took a migration to earn that. Amendments used to write
/// only to `task_amendments` -- no activity row, no `updated_at` bump -- so this
/// had to union the two tables to see the way a worker is meant to record
/// progress. Schema 101 puts amendments in the trail and backfills the ones
/// that predate it, so the union is gone and a future consumer of "has anyone
/// touched this task" has one place to look instead of two to remember.
///
/// Actor kind matters: 'system' rows are the machine talking to itself and
/// jira/email rows are inbound sync, so neither is evidence a person or a
/// worker picked the work up. Counting them would let the coordinator reset
/// its own clock.
///
/// Kind matters too, and the excluded ones are the reason to be explicit
/// rather than to take every row. 'created', 'assigned' and the
/// `state_changed` that moves a task into the state a flag watches are the
/// bookkeeping that makes it eligible in the first place. Counting them would
/// reset the clock at the same moment it starts.
/// When a task BECAME blocked. Transitions only.
///
/// A SECOND CLOCK BESIDE `last_task_action_source!`, ON PURPOSE, and the two
/// answer different questions. That one asks "has anyone acted on this task",
/// where a note IS evidence of engagement and correctly resets it. This one asks
/// "when did this become blocked", where a note is precisely the thing that
/// looks like movement without being it.
///
/// The shared macro exists so definitions cannot drift, so adding a second is a
/// trade rather than a tidy-up. It is the right one here because collapsing the
/// two is what produced the defect: `swarm_correct_task_record` writes
/// `kind='corrected'` with `to_state` set to the task's CURRENT state, so on a
/// blocked task a correction is indistinguishable from a re-block, and `MAX()`
/// takes it.
///
/// MEASURED ON THE LIVE BOARD, not reasoned about. 01a040e4 was blocked for
/// 10.8 hours and read 0.4 after one note; 01a04008 was blocked 4.0 and read
/// 0.4. The operator's twelve-hour escalation exists to reach them when Queen is
/// the bottleneck, and a Queen annotating blocked work -- which is what a
/// conscientious Queen does -- was silencing the alarm built to catch her.
///
/// Filtering by kind rather than by actor, because the actor is not the problem:
/// a worker's correction suppresses it identically.
macro_rules! blocked_transition_source {
    () => {
        "(SELECT task_id, MAX(occurred_at) AS blocked_at
          FROM task_activity
          WHERE to_state = 'blocked' AND kind = 'state_changed'
          GROUP BY task_id)"
    };
}

macro_rules! last_task_action_source {
    () => {
        "(SELECT task_id, MAX(occurred_at) AS acted_at
                        FROM task_activity
                        WHERE actor_kind IN ('worker', 'operator')
                          AND kind IN ('corrected', 'details_updated', 'amended')
                        GROUP BY task_id)"
    };
}

/// Delivered Ready work that nobody has acted on since the briefing landed.
///
/// Lifted out of the function body because the reasoning in it is longer
/// than the query, and clippy caps a function at 100 lines.
const UNSTARTED_WORK_CANDIDATES_SQL: &str = concat!(
    "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at,
                    MAX(0, ?1 - MAX(dispatch.delivered_at, COALESCE(acted.acted_at, 0)))
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN task_assignments assignment
               ON assignment.task_id = task.id AND assignment.released_at IS NULL
             JOIN worker_sessions session
               ON session.session_id = assignment.worker_session_id
                  AND session.worker_id = worker.id AND session.ended_at IS NULL
             JOIN task_dispatches dispatch
               ON dispatch.assignment_id = assignment.id AND dispatch.worker_id = worker.id
                  AND dispatch.state = 'delivered'
             -- THE CLOCK RUNS FROM THE LAST THING ANYONE DID TO THIS TASK, not
             -- from when the briefing was handed over. Those are the same number
             -- only for a task nobody has touched, which is the one case this
             -- flag is actually about.
             --
             -- Delivered-at alone asks 'has this been transitioned to Active
             -- yet', and treats that as a proxy for 'is anyone working it'. It
             -- is not one. A worker can work a Ready task for an hour --
             -- amending its facts, correcting the record -- and never transition
             -- it, and the proxy reports it as ignored the whole time. Eleven of
             -- the twelve instances Queen recorded were that exact shape, and
             -- reading each one cost a transcript.
             --
             -- ⚠️ THIS SENTENCE ALSO SAID `leaving notes` UNTIL 2026-09-03 AND
             -- CODE HAS NEVER DONE THAT. `noted` is deliberately absent from
             -- last_task_action_source!, defended by
             -- a_note_does_not_hold_off_the_stale_flag_although_an_amendment_does,
             -- because a note is cheap and would become a way to look busy.
             --
             -- So a comment and a test stated opposite intentions, both in this
             -- file, with no way for a reader to tell which was authoritative
             -- without running the suite. A worker read this comment, made the
             -- one-word change it implies, and was caught only because the
             -- existing test asserts the opposite. The comment was the wrong
             -- half: the test is the decision somebody made on purpose.

             LEFT JOIN ",
    last_task_action_source!(),
    " acted
               ON acted.task_id = task.id
             WHERE task.state = 'ready' AND dispatch.delivered_at IS NOT NULL
               AND MAX(dispatch.delivered_at, COALESCE(acted.acted_at, 0)) + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker.id AND engagement.expires_at > ?1
               )
               -- A worker carrying OTHER active work has not ignored this
               -- briefing; it is queued behind the thing that worker is doing.
               -- Without this the attention list and the held-briefing list
               -- describe the same situation and disagree about whether it is a
               -- problem: delivery already holds a briefing back for exactly
               -- this reason and reports it as worker_already_working, while
               -- this flag called it delivered-but-never-started.
               --
               -- Queen hit it three times in one night on one task and had to
               -- read a transcript and /proc each time to establish that nothing
               -- was wrong. A flag that cannot tell queued-behind-higher-
               -- priority-work from delivered-and-ignored trains its reader
               -- to check every instance by hand, which is the same as not
               -- having it, and worse, because reaching that conclusion costs
               -- attention every time.
               AND NOT EXISTS (
                   SELECT 1 FROM tasks active
                   WHERE active.assigned_worker_id = worker.id
                     AND active.state = 'active' AND active.removed_at IS NULL
                     AND active.id <> task.id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'assigned_ready_work_not_started_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY MAX(dispatch.delivered_at, COALESCE(acted.acted_at, 0)),
                      task.id LIMIT ?3"
);

/// The one definition of a coordination-attention record that is still true.
///
/// Three questions are asked of it: what Queen sees when she reviews, how much
/// is actionable, and — crucially — the fingerprint that decides she should run
/// at all. They were three copies of one predicate, and a fourth kind added to
/// only the first was missed by the other two. The result was exactly what the
/// operator saw: a worker filed several tasks, the record existed, Queen could
/// have read it, and nothing ever woke her to look.
///
/// Every branch re-verifies against live state, so a record whose reason has
/// passed stops counting without anything having to delete it.
pub(super) const LIVE_ATTENTION_SOURCE: &str = "FROM coordinator_actions action
             JOIN tasks task ON task.id = action.task_id
             JOIN worker_profiles worker ON worker.id = action.worker_id
             JOIN worker_sessions session ON session.session_id = action.session_id
             WHERE action.kind IN ('stale_owned_work_attention','owned_work_never_briefed_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','reviewed_work_without_evidence_attention','blocked_work_unattended_attention','evidenced_work_not_closed_attention')
               AND action.state = 'completed'
               AND task.updated_at = action.evidence_revision
               AND session.worker_id = action.worker_id
               AND (
                   (action.kind = 'worker_filed_draft_attention'
                       AND task.state = 'draft' AND task.assigned_worker_id IS NULL)
                   -- The decision this was raised for is named in the
                   -- idempotency key rather than in a column, so the joins
                   -- above are untouched: the row still hangs off the worker
                   -- that asked and the task it concerns. It counts only while
                   -- that decision is genuinely still waiting, so answering it
                   -- clears the attention without anything having to delete it.
                   OR (action.kind = 'decision_deadline_passed_attention'
                       AND EXISTS (
                           SELECT 1 FROM decision_requests request
                           WHERE 'decision-deadline:' || request.id = action.idempotency_key
                             AND request.state = 'pending'
                             AND request.deadline IS NOT NULL
                             AND request.deadline <= unixepoch()
                       ))
                   -- Finished work nobody can close. Clears itself the moment
                   -- real evidence exists, or the task leaves review, so
                   -- settling it is the whole fix and nothing has to be
                   -- dismissed by hand.
                   --
                   -- AN UNAPPROVED CLAIM IS NOT EVIDENCE, and this guard used
                   -- to treat it as though it were: it matched the exemption
                   -- row's existence and ignored `approved_at`. So the instant
                   -- a worker called `swarm_record_no_deployment` the task
                   -- disappeared from Queen's attention -- silenced by the very
                   -- act that created the thing needing her approval. She
                   -- approves within minutes when she is still in the
                   -- conversation at claim time and never afterwards, because
                   -- afterwards nothing tells her. On 2026-08-31 that left
                   -- eight claims invisible to her and visible only on the
                   -- operator's card, which is not where they get settled.
                   --
                   -- `completion_evidence` already draws this line: a claim is
                   -- `ExemptionClaimed`, which `closes_a_task` deliberately
                   -- refuses. The detector now draws it in the same place.
                   OR (action.kind = 'reviewed_work_without_evidence_attention'
                       AND task.state = 'review'
                       AND NOT EXISTS (
                           SELECT 1 FROM task_deployments deployment
                           WHERE deployment.task_id = task.id
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM task_completion_exemptions exemption
                           WHERE exemption.task_id = task.id
                             AND exemption.approved_at IS NOT NULL AND exemption.withdrawn_at IS NULL
                       ))
                   -- Clears itself by the task being CLOSED, which is the
                   -- only act that answers it. Approving the evidence is what
                   -- created this record, so re-checking the evidence here
                   -- would make it permanently true and permanently unactioned.
                   OR (action.kind = 'evidenced_work_not_closed_attention'
                       AND task.state = 'review'
                       AND (EXISTS (
                               SELECT 1 FROM task_deployments deployment
                               WHERE deployment.task_id = task.id
                           )
                           OR EXISTS (
                               SELECT 1 FROM task_completion_exemptions exemption
                               WHERE exemption.task_id = task.id
                                 AND exemption.approved_at IS NOT NULL AND exemption.withdrawn_at IS NULL
                           )))
                   OR (action.kind = 'stale_owned_work_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'active' AND session.ended_at IS NULL)
                   -- Clears itself the moment a brief is confirmed delivered,
                   -- so redelivering the work is the whole fix and nothing has
                   -- to be dismissed by hand.
                   OR (action.kind = 'owned_work_never_briefed_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'active' AND session.ended_at IS NULL
                       AND NOT EXISTS (
                           SELECT 1 FROM task_dispatches delivered
                           WHERE delivered.task_id = action.task_id
                             AND delivered.worker_id = action.worker_id
                             AND delivered.delivered_at IS NOT NULL
                       ))
                   OR (action.kind = 'owned_work_worker_exited_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'active'
                       AND session.ended_at IS NOT NULL
                       AND session.session_id = (
                           SELECT latest.session_id FROM worker_sessions latest
                           WHERE latest.worker_id = action.worker_id
                             AND latest.ended_at IS NOT NULL
                           ORDER BY latest.ended_at DESC, latest.started_at DESC, latest.session_id DESC
                           LIMIT 1
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM worker_sessions live
                           WHERE live.worker_id = action.worker_id AND live.ended_at IS NULL
                       ))
                   -- Counts only while the task is STILL blocked. Queen moving
                   -- it is what clears this, with nothing having to delete the
                   -- row -- and Queen moving it is the entire point.
                   OR (action.kind = 'blocked_work_unattended_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'blocked')
                   OR (action.kind = 'assigned_ready_work_not_started_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'ready' AND session.ended_at IS NULL
                       -- AND the reason has passed once the worker does
                       -- anything to the task itself. The revision check above
                       -- cannot see this on its own: an amendment moves neither
                       -- tasks.updated_at nor the activity trail, so a row
                       -- raised a minute before the worker started work stayed
                       -- on the board for as long as the work took. Guarding
                       -- creation alone would have left exactly that behind,
                       -- which is the mistake the note below records being made
                       -- once already.
                       AND NOT EXISTS (
                           SELECT 1 FROM task_amendments amendment
                           WHERE amendment.task_id = task.id
                             AND amendment.created_at >= COALESCE(action.finished_at, action.created_at)
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM task_activity acted
                           WHERE acted.task_id = task.id
                             AND acted.actor_kind IN ('worker', 'operator')
                             AND acted.kind IN ('corrected', 'details_updated')
                             AND acted.occurred_at >= COALESCE(action.finished_at, action.created_at)
                       )
                       -- The reason has passed once its worker picks up other
                       -- work: the briefing is queued behind that, not ignored.
                       -- Guarding only where the row is CREATED stops new false
                       -- rows and leaves existing ones surfacing forever, which
                       -- is what a Queen hit four automation runs in a row --
                       -- watching a live-computed age climb past an hour on a
                       -- situation that was never a problem.
                       AND NOT EXISTS (
                           SELECT 1 FROM tasks busy
                           WHERE busy.assigned_worker_id = action.worker_id
                             AND busy.state = 'active' AND busy.removed_at IS NULL
                             AND busy.id <> task.id
                       )
                       AND EXISTS (
                           SELECT 1 FROM task_assignments assignment
                           JOIN task_dispatches dispatch
                             ON dispatch.assignment_id = assignment.id
                                AND dispatch.state = 'delivered'
                           WHERE assignment.task_id = task.id
                             AND dispatch.worker_id = action.worker_id
                             AND assignment.worker_session_id = action.session_id
                             AND assignment.released_at IS NULL
                       ))
               )";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorWorkerWake {
    pub action_id: String,
    pub worker_id: WorkerId,
    pub task_id: TaskId,
}

/// Adds what the worker HAS recorded to a row saying nothing has changed.
///
/// A note deliberately buys no quiet — see
/// `a_note_does_not_hold_off_the_stale_flag_although_an_amendment_does` — so the
/// age keeps climbing while a worker records progress. That is correct, and it
/// is not what a coordinator reads. Queen read 9002 seconds on one row and asked
/// a worker whether it had stalled; it had not, it was two hours into shipping
/// seven of nine subsystems and had written 32 notes, the newest a minute old.
///
/// Her own diagnosis is the design: "the row's prose is right while its number
/// is wrong — the number is what makes a coordinator ask." So the number stays
/// honest and stops being the only thing on the row.
fn note_evidence_beside(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    now: i64,
    reason: &str,
) -> Result<String, TaskStoreError> {
    let (count, newest): (i64, Option<i64>) = transaction.query_row(
        "SELECT count(*), MAX(occurred_at) FROM task_activity
         WHERE task_id = ?1 AND kind = 'noted'",
        [task_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let Some(newest) = newest.filter(|_| count > 0) else {
        return Ok(reason.to_owned());
    };
    Ok(format!(
        "{reason} The worker has recorded {count} note{} on it, the newest {}. \
         Notes do not clear this flag on purpose — read them before deciding it stalled.",
        if count == 1 { "" } else { "s" },
        describe_age(now.saturating_sub(newest)),
    ))
}

/// How long ago, for a coordinator reading a row rather than a log.
///
/// Rounded and in words on purpose: the point of this number is whether to
/// interrupt somebody, and "a minute ago" answers that where 63 seconds asks
/// the reader to do arithmetic first.
fn describe_age(seconds: i64) -> String {
    match seconds {
        ..=90 => "moments ago".to_owned(),
        _ if seconds < 5_400 => format!("{} minutes ago", (seconds + 30) / 60),
        _ => format!("{} hours ago", (seconds + 1_800) / 3_600),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleOwnedWorkCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitedWorkerOwnedWorkCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

/// Work whose evidence is settled and which nobody then closed.
///
/// The gap this fills is narrow and was invisible for nine hours on a real
/// board. `reviewed_work_without_evidence_attention` chases work in review
/// with no APPROVED evidence, and it deliberately stops the moment an
/// exemption is approved — approving is the answer to that question. But
/// approval only records evidence; it does not close the task, by design,
/// because Queen still has to judge the work and not merely its paperwork.
///
/// So between "approved" and "closed" there was no watcher at all, and a task
/// sitting there reads as settled from every angle: it has evidence, so the
/// evidence detector skips it, and it is not completed, so nothing counts it
/// as done.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidencedWorkNotClosedCandidate {
    pub worker_id: WorkerId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
    pub session_id: WorkerSessionId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnattendedBlockCandidate {
    pub worker_id: WorkerId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
    /// The worker's most recent session, whether or not it is still running.
    ///
    /// A blocked worker is frequently ASLEEP, so requiring a live session would
    /// exclude exactly the tasks that have waited longest — the ones this exists
    /// to surface.
    pub session_id: WorkerSessionId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignedReadyWorkNotStartedCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

/// Finished work that cannot be closed, because neither kind of completion
/// evidence exists for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewedWorkWithoutEvidenceCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorAttention {
    pub action_id: String,
    pub kind: String,
    pub worker_id: WorkerId,
    pub worker_name: String,
    pub task_id: TaskId,
    pub task_title: String,
    pub reason: String,
    pub observed_at: i64,
    /// How long this has been standing, computed when the row is read.
    ///
    /// It used to be `observed_age_seconds`, written once at observation and
    /// returned verbatim ever after, so it read like a live age and was not
    /// one. Measured 2026-08-23: three records reported 301, 303 and 310
    /// seconds while the conditions were 176 to 178 minutes old — understated
    /// by a factor of thirty — and one value moved DOWNWARD between two calls,
    /// so it was inconsistent as well as stale.
    ///
    /// The harm landed. Queen read 303s, concluded the briefs were five
    /// minutes old and therefore not yet evidence of a stall, and wrote that
    /// recommendation into a live operator decision record. At the real age she
    /// would have restarted the worker.
    pub age_seconds: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoordinatorStatus {
    pub completed_actions: usize,
    pub queen_calls_avoided: usize,
    pub uncertain_actions: usize,
    pub queued_actions: usize,
    pub stale_attention_actions: usize,
    pub worker_exit_attention_actions: usize,
    pub unstarted_attention_actions: usize,
    pub last_action_at: Option<i64>,
}

pub(crate) fn enqueue_queen_worker_wake(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    worker_id: WorkerId,
    actor_id: Option<&str>,
    assignment_sequence: i64,
    task_state: &str,
    worker_is_sleeping: bool,
) -> Result<bool, TaskStoreError> {
    if task_state != "ready" || !worker_is_sleeping {
        return Ok(false);
    }
    let Some(actor_id) = actor_id else {
        return Ok(false);
    };
    let actor_is_queen: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_profiles WHERE id = ?1 AND role = 'queen' AND archived_at IS NULL)",
        [actor_id],
        |row| row.get(0),
    )?;
    if !actor_is_queen {
        return Ok(false);
    }
    let idempotency_key =
        format!("wake-assigned-worker:{task_id}:{worker_id}:{assignment_sequence}");
    let changed = transaction.execute(
        "INSERT OR IGNORE INTO coordinator_actions
             (id, idempotency_key, kind, worker_id, task_id, state, reason)
         VALUES (?1, ?2, 'wake_assigned_worker', ?3, ?4, 'queued',
                 'Queen assigned Ready work to a sleeping worker')",
        params![
            Uuid::now_v7().to_string(),
            idempotency_key,
            worker_id.to_string(),
            task_id.to_string(),
        ],
    )? == 1;
    if changed {
        insert_control_room_event(transaction, ControlRoomEventKind::WorkersChanged)?;
    }
    Ok(changed)
}

/// What could be established about work the provider left running.
///
/// THREE ANSWERS, BECAUSE THERE ARE THREE. The detector greps the worker's
/// TERMINAL SCREEN for the provider's own banner -- "2 shells still running"
/// and its siblings. It cannot observe a process, so the strongest true
/// statement it can ever make is about what the terminal shows.
///
/// Collapsing the last two into `false` is what produced a row reading "nothing
/// it started is still running" beside a `cargo test` that had been going for
/// two and a half minutes. The flag was not wrong about what it measured; the
/// SENTENCE was wrong about what the flag meant, and a coordinator reading it
/// spent a real round of investigation on a healthy worker. Twice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundWorkReading {
    /// The terminal shows the provider's background-work banner.
    Running,
    /// The terminal was read and shows no such banner. NOT "nothing is
    /// running": a process the provider never announced is invisible here.
    NoneVisible,
    /// The terminal could not be read. Nothing was measured, so nothing is
    /// claimed -- the case that used to render as a confident negative.
    Unreadable,
}

impl TaskStore {
    /// Returns bounded durable candidates whose worker process ended while it
    /// still owned Active work. The newest ended session is the exact process
    /// incarnation bound into the observation; a replacement live session
    /// suppresses the candidate.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn exited_worker_owned_work_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<ExitedWorkerOwnedWorkCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at, MAX(0, ?1 - session.ended_at)
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN worker_sessions session ON session.session_id = (
                 SELECT latest.session_id FROM worker_sessions latest
                 WHERE latest.worker_id = worker.id AND latest.ended_at IS NOT NULL
                 ORDER BY latest.ended_at DESC, latest.started_at DESC, latest.session_id DESC
                 LIMIT 1
             )
             WHERE task.state = 'active' AND session.ended_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions live
                   WHERE live.worker_id = worker.id AND live.ended_at IS NULL
               )
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker.id AND engagement.expires_at > ?1
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'owned_work_worker_exited_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY session.ended_at, task.id LIMIT ?3",
        )?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_EXITED_WORK_CANDIDATES],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .map(|row| {
                let (worker_id, session_id, task_id, task_revision, age_seconds) = row?;
                Ok::<_, rusqlite::Error>(ExitedWorkerOwnedWorkCandidate {
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: session_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_revision,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Records one exact worker-exit observation after the grace period. The
    /// task revision, owner, ended session, lack of a replacement session, and
    /// lack of operator engagement are rechecked atomically.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_exited_worker_owned_work_attention(
        &self,
        candidate: &ExitedWorkerOwnedWorkCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN worker_sessions session ON session.session_id = ?4
                 WHERE task.id = ?1 AND task.state = 'active'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND session.worker_id = ?2 AND session.ended_at IS NOT NULL
                   AND session.ended_at + ?5 <= ?6
                   AND session.session_id = (
                       SELECT latest.session_id FROM worker_sessions latest
                       WHERE latest.worker_id = ?2 AND latest.ended_at IS NOT NULL
                       ORDER BY latest.ended_at DESC, latest.started_at DESC, latest.session_id DESC
                       LIMIT 1
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_sessions live
                       WHERE live.worker_id = ?2 AND live.ended_at IS NULL
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements engagement
                       WHERE engagement.worker_id = ?2 AND engagement.expires_at > ?6
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                candidate.session_id.to_string(),
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_current {
            transaction.commit()?;
            return Ok(false);
        }
        let idempotency_key = format!(
            "owned-work-worker-exited:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'owned_work_worker_exited_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'Active work lost its loaded worker after the process exited',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns delivered Ready assignments whose loaded worker has remained
    /// resting without starting the task. Runtime/provider evidence is
    /// deliberately evaluated by the API before attention is recorded.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn assigned_ready_work_not_started_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<AssignedReadyWorkNotStartedCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(UNSTARTED_WORK_CANDIDATES_SQL)?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .map(|row| {
                let (worker_id, session_id, task_id, task_revision, age_seconds) = row?;
                Ok::<_, rusqlite::Error>(AssignedReadyWorkNotStartedCandidate {
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: session_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_revision,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// A block old enough that the operator should hear about it directly.
    ///
    /// The operator chose twelve hours over Queen's recommended twenty-four
    /// (decision 01a0418f). Their report was that Queen never changes anything, so
    /// surfacing an aged block to Queen alone tells the party that was already
    /// silent -- this reaches past her.
    ///
    /// VISIBILITY, NOT AUTHORITY. Nothing here moves a task. Queen remains the only
    /// actor that takes work out of Blocked, which the operator asked for in those
    /// words, and the escalation is a listing rather than a transition.
    ///
    /// AGE COMES FROM `task_activity`, NOT `updated_at`, for the same reason the
    /// four-hour version does: a sweep that touches the row resets `updated_at` and
    /// would make a week-old block look new.
    ///
    /// A BLOCK THAT NAMED A FUTURE MOMENT IS NOT ESCALATED UNTIL IT ARRIVES, and
    /// that clause is the whole design rather than a refinement. Without it the
    /// first two things this ever says to the operator are both correct and
    /// unactionable -- two tasks waiting on a zero-traffic window with hours left --
    /// and a channel whose opening messages cannot be acted on is one its reader
    /// learns to dismiss. A deadline that has ELAPSED escalates: the reason expired
    /// and nobody came back.
    ///
    /// A block waiting on a decision still escalates, answered or not. Pending
    /// means the operator answering it IS the action; answered means Queen has the
    /// answer and has not moved, which is exactly what was reported.
    ///
    /// # Errors
    /// Returns an error when the query fails.
    pub fn operator_block_escalation_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<UnattendedBlockCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(concat!(
            "SELECT task.assigned_worker_id, task.id, task.updated_at,
                    MAX(0, ?1 - blocked.blocked_at), session.session_id
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN ",
            blocked_transition_source!(),
            " blocked
               ON blocked.task_id = task.id
             JOIN (SELECT worker_id, session_id,
                          ROW_NUMBER() OVER (PARTITION BY worker_id
                                             ORDER BY started_at DESC) AS recency
                   FROM worker_sessions) session
               ON session.worker_id = worker.id AND session.recency = 1
             WHERE task.state = 'blocked' AND task.removed_at IS NULL
               AND blocked.blocked_at + ?2 <= ?1
               AND (task.blocked_until IS NULL OR task.blocked_until <= ?1)
             ORDER BY blocked.blocked_at, task.id LIMIT ?3"
        ))?;
        let candidates = statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
                |row| {
                    Ok(UnattendedBlockCandidate {
                        worker_id: WorkerId::from_str(&row.get::<_, String>(0)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_id: TaskId::from_str(&row.get::<_, String>(1)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_revision: row.get(2)?,
                        age_seconds: row.get(3)?,
                        session_id: WorkerSessionId::from_str(&row.get::<_, String>(4)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates)
    }

    /// Work in review whose evidence is settled and which was never closed.
    ///
    /// Keyed on the same disjunction `closed_on_evidence` is derived from, so
    /// this asks exactly the question the board's own flag answers — with the
    /// state it does NOT check bolted on: still in review.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn evidenced_work_not_closed_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<EvidencedWorkNotClosedCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, task.id, task.updated_at,
                    MAX(0, ?1 - task.updated_at), session.session_id
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN (SELECT worker_id, session_id,
                          ROW_NUMBER() OVER (PARTITION BY worker_id
                                             ORDER BY started_at DESC) AS recency
                   FROM worker_sessions) session
               ON session.worker_id = worker.id AND session.recency = 1
             WHERE task.state = 'review' AND task.removed_at IS NULL
               AND task.updated_at + ?2 <= ?1
               AND (EXISTS (
                       SELECT 1 FROM task_deployments deployment
                       WHERE deployment.task_id = task.id
                   )
                   OR EXISTS (
                       SELECT 1 FROM task_completion_exemptions exemption
                       WHERE exemption.task_id = task.id
                         AND exemption.approved_at IS NOT NULL AND exemption.withdrawn_at IS NULL
                   ))
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'evidenced_work_not_closed_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY task.updated_at, task.id LIMIT ?3",
        )?;
        let candidates = statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
                |row| {
                    Ok(EvidencedWorkNotClosedCandidate {
                        worker_id: WorkerId::from_str(&row.get::<_, String>(0)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_id: TaskId::from_str(&row.get::<_, String>(1)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_revision: row.get(2)?,
                        age_seconds: row.get(3)?,
                        session_id: WorkerSessionId::from_str(&row.get::<_, String>(4)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates)
    }

    /// Records one settled-but-unclosed observation.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn record_evidenced_work_not_closed_attention(
        &self,
        candidate: &EvidencedWorkNotClosedCandidate,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let idempotency_key = format!(
            "evidenced-work-not-closed:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'evidenced_work_not_closed_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed',
                     'Finished work has approved evidence and was never closed',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// A blocked task nobody has come back to, and how long it has waited.
    ///
    /// AGE COMES FROM `task_activity`, NOT `updated_at`, and that is the whole
    /// reason this reports anything. When this was measured against the live
    /// database, `updated_at` said 0.6 hours for EVERY blocked task because a
    /// sweep had touched them — while the oldest had been blocked 168.6 hours.
    /// A query written the obvious way reports a healthy board.
    ///
    /// # Errors
    /// Returns an error when the query fails.
    pub fn unattended_block_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<UnattendedBlockCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(concat!(
            "SELECT task.assigned_worker_id, task.id, task.updated_at,
                    MAX(0, ?1 - blocked.blocked_at), session.session_id
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN ",
            blocked_transition_source!(),
            " blocked
               ON blocked.task_id = task.id
             JOIN (SELECT worker_id, session_id,
                          ROW_NUMBER() OVER (PARTITION BY worker_id
                                             ORDER BY started_at DESC) AS recency
                   FROM worker_sessions) session
               ON session.worker_id = worker.id AND session.recency = 1
             WHERE task.state = 'blocked' AND task.removed_at IS NULL
               AND blocked.blocked_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'blocked_work_unattended_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY blocked.blocked_at, task.id LIMIT ?3"
        ))?;
        let candidates = statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
                |row| {
                    Ok(UnattendedBlockCandidate {
                        worker_id: WorkerId::from_str(&row.get::<_, String>(0)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_id: TaskId::from_str(&row.get::<_, String>(1)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_revision: row.get(2)?,
                        age_seconds: row.get(3)?,
                        session_id: WorkerSessionId::from_str(&row.get::<_, String>(4)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates)
    }

    /// Records that a blocked task has waited without anyone acting on it.
    ///
    /// Records only. It moves nothing: Queen remains the only actor that takes a
    /// task out of Blocked, which is the constraint the operator set.
    ///
    /// # Errors
    /// Returns an error when the write fails.
    pub fn record_unattended_block_attention(
        &self,
        candidate: &UnattendedBlockCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_blocked: bool = transaction.query_row(
            concat!(
                "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN ",
                blocked_transition_source!(),
                " blocked ON blocked.task_id = task.id
                 WHERE task.id = ?1 AND task.state = 'blocked'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND task.removed_at IS NULL
                   AND blocked.blocked_at + ?4 <= ?5
             )"
            ),
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_blocked {
            transaction.commit()?;
            return Ok(false);
        }
        let idempotency_key = format!(
            "blocked-work-unattended:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'blocked_work_unattended_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'Blocked work has waited without anyone acting on it',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Records one exact unstarted-work observation after the delivered brief
    /// has exceeded its grace period. Assignment, session, revision, delivery,
    /// and lack of operator engagement are rechecked atomically.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_assigned_ready_work_not_started_attention(
        &self,
        candidate: &AssignedReadyWorkNotStartedCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN task_assignments assignment
                   ON assignment.task_id = task.id AND assignment.released_at IS NULL
                 JOIN worker_sessions session
                   ON session.session_id = assignment.worker_session_id
                      AND session.worker_id = ?2 AND session.ended_at IS NULL
                 JOIN task_dispatches dispatch
                   ON dispatch.assignment_id = assignment.id AND dispatch.worker_id = ?2
                      AND dispatch.state = 'delivered'
                 WHERE task.id = ?1 AND task.state = 'ready'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND session.session_id = ?4 AND dispatch.delivered_at IS NOT NULL
                   AND dispatch.delivered_at + ?5 <= ?6
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements engagement
                       WHERE engagement.worker_id = ?2 AND engagement.expires_at > ?6
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                candidate.session_id.to_string(),
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_current {
            transaction.commit()?;
            return Ok(false);
        }
        let idempotency_key = format!(
            "assigned-ready-work-not-started:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'assigned_ready_work_not_started_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'Ready work was delivered but its loaded worker did not start it',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Work in review that has neither a deployment nor a no-deployment claim.
    ///
    /// Unlike every other kind here, the session is not required to be live:
    /// the worst version of this is exactly the one where the worker has moved
    /// on, because then nothing else will ever record the claim.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn reviewed_work_without_evidence_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<ReviewedWorkWithoutEvidenceCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at, MAX(0, ?1 - task.updated_at)
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN task_assignments assignment
               ON assignment.task_id = task.id
             JOIN worker_sessions session
               ON session.session_id = assignment.worker_session_id
                  AND session.worker_id = worker.id
             WHERE task.state = 'review' AND task.removed_at IS NULL
               AND task.updated_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM task_deployments deployment
                   WHERE deployment.task_id = task.id
               )
               -- Approved, not merely claimed. A worker cannot approve its own
               -- exemption, so matching the row's existence let the claim
               -- suppress the detector that exists to get the claim approved.
               AND NOT EXISTS (
                   SELECT 1 FROM task_completion_exemptions exemption
                   WHERE exemption.task_id = task.id
                     AND exemption.approved_at IS NOT NULL AND exemption.withdrawn_at IS NULL
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = \'reviewed_work_without_evidence_attention\'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             GROUP BY task.id
             ORDER BY task.updated_at, task.id LIMIT ?3",
        )?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .map(|row| {
                let (worker_id, session_id, task_id, task_revision, age_seconds) = row?;
                Ok::<_, rusqlite::Error>(ReviewedWorkWithoutEvidenceCandidate {
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: session_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_revision,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Records one exact observation of finished work with no evidence.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_reviewed_work_without_evidence_attention(
        &self,
        candidate: &ReviewedWorkWithoutEvidenceCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 WHERE task.id = ?1 AND task.state = \'review\'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND task.updated_at + ?4 <= ?5
                   AND NOT EXISTS (
                       SELECT 1 FROM task_deployments deployment
                       WHERE deployment.task_id = task.id
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM task_completion_exemptions exemption
                       WHERE exemption.task_id = task.id
                         AND exemption.approved_at IS NOT NULL AND exemption.withdrawn_at IS NULL
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_current {
            transaction.commit()?;
            return Ok(false);
        }
        let idempotency_key = format!(
            "reviewed-work-without-evidence:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, \'reviewed_work_without_evidence_attention\', ?3, ?4, ?5, ?6, ?7,
                     \'completed\', \'Work reached review with neither a deployment nor a no-deployment claim\',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Tells Queen that a worker filed work and cannot route it.
    ///
    /// A worker has no channel to Queen at all: her inbox is written by
    /// detectors and nothing else, so a worker wanting work routed had to
    /// interrupt the operator instead — the opposite of what Queen is for. It
    /// can file a draft, but nothing said one was waiting.
    ///
    /// Recorded when the draft is filed rather than found by a later sweep,
    /// because the filing is the event. One record per task, and it stops being
    /// shown the moment the task is no longer an unrouted draft.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_worker_filed_draft_attention(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
        session_id: WorkerSessionId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let revision: i64 = transaction.query_row(
            "SELECT updated_at FROM tasks WHERE id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        // Deliberately not INSERT OR IGNORE on the conflict clause alone: that
        // suppresses a CHECK violation exactly as it suppresses a duplicate, so
        // an unadmitted kind would do nothing and say nothing. Only a repeat of
        // the same filing is ignorable.
        let changed = transaction.execute(
            "INSERT INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'worker_filed_draft_attention', ?3, ?4, ?5, ?6, 0,
                     'completed', 'A worker filed this work and cannot route it',
                     unixepoch(), unixepoch())
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                Uuid::now_v7().to_string(),
                format!("worker-filed-draft:{task_id}"),
                worker_id.to_string(),
                task_id.to_string(),
                session_id.to_string(),
                revision,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns bounded durable candidates for stale-owned-work observation.
    /// Runtime/provider evidence is deliberately evaluated by the API before
    /// any attention action is recorded.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn stale_owned_work_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<StaleOwnedWorkCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(concat!(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at,
                    MAX(0, ?1 - MAX(task.updated_at, COALESCE(acted.acted_at, 0)))
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN worker_sessions session
               ON session.worker_id = worker.id AND session.ended_at IS NULL
             -- updated_at ALONE MISSES THE ONE THING A WORKING WORKER DOES.
             -- Transitions move it; amendments do not. So a worker steadily
             -- recording progress on an Active task -- the behaviour the board
             -- asks for -- read as untouched for as long as it kept doing it.
             -- Same blind spot the unstarted-work flag had, same answer.
             LEFT JOIN ",
            last_task_action_source!(),
            " acted
               ON acted.task_id = task.id
             WHERE task.state = 'active'
               AND MAX(task.updated_at, COALESCE(acted.acted_at, 0)) + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker.id AND engagement.expires_at > ?1
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'stale_owned_work_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
               -- Work stops changing while its answer is with the operator, and
               -- that is the system working rather than a fault. Reporting it as
               -- unchanged sends the operator to look at a worker that is
               -- already waiting on them.
               AND NOT EXISTS (
                   SELECT 1 FROM decision_requests decision
                   WHERE decision.task_id = task.id AND decision.state = 'pending'
               )
             ORDER BY MAX(task.updated_at, COALESCE(acted.acted_at, 0)),
                      task.id LIMIT ?3"
        ))?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_STALE_CANDIDATES],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .map(|row| {
                let (worker_id, session_id, task_id, task_revision, age_seconds) = row?;
                Ok::<_, rusqlite::Error>(StaleOwnedWorkCandidate {
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: session_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_revision,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Records one exact stale-owned-work observation after provider activity
    /// confirmed that the loaded worker was resting. All durable preconditions
    /// are rechecked atomically so a concurrent task or engagement change wins.
    ///
    /// # Errors
    /// Returns a persistence error.
    /// `background_work` is whether something the worker STARTED is still
    /// running — a build, a `gh run watch`, a measurement loop — while its
    /// prompt rests. The classifier deliberately lets a resting prompt outrank
    /// a background shell, so both situations arrive here as a resting worker
    /// on unchanged work, and reporting them identically cost Queen three
    /// rounds of hand-verification in one night: each time she had to open the
    /// worker's repository to find out that waiting WAS the work.
    ///
    /// It changes the reason rather than the kind on purpose. A new kind would
    /// have to be added to the CHECK constraint, to `LIVE_ATTENTION_SOURCE`, and
    /// — the one that actually bites — to the NOT EXISTS in
    /// `stale_owned_work_candidates`, which dedupes on
    /// `kind = 'stale_owned_work_attention'`. Miss that last one and the
    /// candidate is re-selected and re-flagged every tick, which is worse than
    /// the false positive being fixed.
    ///
    /// It is still FLAGGED, not suppressed. A worker resting beside a `sleep`
    /// loop it forgot about is genuinely stalled, and a detector that went
    /// quiet whenever any process was alive would never say so.
    pub fn record_stale_owned_work_attention(
        &self,
        candidate: &StaleOwnedWorkCandidate,
        now: i64,
        minimum_age_seconds: i64,
        background_work: BackgroundWorkReading,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN worker_sessions session
                   ON session.worker_id = task.assigned_worker_id AND session.ended_at IS NULL
                 WHERE task.id = ?1 AND task.state = 'active'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND session.session_id = ?4 AND task.updated_at + ?5 <= ?6
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements engagement
                       WHERE engagement.worker_id = task.assigned_worker_id
                         AND engagement.expires_at > ?6
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                candidate.session_id.to_string(),
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_current {
            transaction.commit()?;
            return Ok(false);
        }
        // Whether this worker was ever actually handed the work.
        //
        // The two cases look identical from outside — no commits, no files
        // touched, a resting worker on Active work — so this reported both as
        // "unchanged while resting", which names the worker. Measured
        // 2026-08-19: a high-priority brief sat with one attempt and no
        // delivery for 27 hours while the worker was blamed for being idle. It
        // had nothing to act on.
        //
        // The dispatch row carried the answer the whole time; nothing consulted
        // it. A brief that was never confirmed delivered is a different problem
        // with a different fix, so it is a different kind of attention.
        //
        // Only an `uncertain` row counts, because only that one is provably
        // going nowhere: queued and dispatching are still in flight, and the
        // absence of any row proves nothing at all — old rows are trimmed once
        // there are more than 1024 of them, so inferring "never briefed" from a
        // missing row would invent the same false accusation in the other
        // direction.
        let never_briefed: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM task_dispatches dispatch
                 WHERE dispatch.task_id = ?1 AND dispatch.worker_id = ?2
                   AND dispatch.state = 'uncertain' AND dispatch.delivered_at IS NULL
             ) AND NOT EXISTS(
                 SELECT 1 FROM task_dispatches delivered
                 WHERE delivered.task_id = ?1 AND delivered.worker_id = ?2
                   AND delivered.delivered_at IS NOT NULL
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string()
            ],
            |row| row.get(0),
        )?;
        let (kind, reason, key) = if never_briefed {
            (
                "owned_work_never_briefed_attention",
                "This worker was never given this brief — the delivery was never confirmed, so it has nothing to act on. Re-assign the task to send it again rather than steering the worker.",
                "owned-work-never-briefed",
            )
        } else if background_work == BackgroundWorkReading::Running {
            (
                "stale_owned_work_attention",
                "Active work is unchanged while its loaded worker rests, but something the worker started is still running — a build, a run watch, or a measurement. Waiting may be the work. Read the terminal before steering.",
                "stale-owned-work",
            )
        } else {
            (
                "stale_owned_work_attention",
                // SAYS WHAT WAS MEASURED, and no more. The old sentence read
                // "nothing it started is still running", which is a claim about
                // processes made by something that only ever looked at a screen.
                if background_work == BackgroundWorkReading::NoneVisible {
                    "Active work is unchanged while its loaded worker is resting, and its terminal shows nothing running. A process the provider never announced would not appear here."
                } else {
                    "Active work is unchanged while its loaded worker is resting, and its terminal could not be read — whether anything it started is still running is unknown."
                },
                "stale-owned-work",
            )
        };
        let reason = note_evidence_beside(&transaction, candidate.task_id, now, reason)?;
        let idempotency_key = format!(
            "{key}:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, ?9, ?3, ?4, ?5, ?6, ?7,
                     'completed', ?10,
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
                kind,
                reason,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Lists current stale-work attention whose task revision, owner, and
    /// worker incarnation still match the observation.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn current_coordinator_attention(
        &self,
        now: i64,
    ) -> Result<Vec<CoordinatorAttention>, TaskStoreError> {
        let connection = self.connection()?;
        // Derived here rather than read from the row it was written on. A
        // cached age is indistinguishable from a live one in the payload, and
        // every conclusion drawn from it inherits the error silently.
        //
        // The sum of both halves, not just the time since. The condition was
        // already `observed_age_seconds` old when it was first noticed — a
        // detector with a grace period never fires at zero — so reporting only
        // the elapsed time would understate it by that grace on every read.
        // What Queen needs is how long the situation has been true.
        let mut statement = connection.prepare(&format!(
            "SELECT action.id, action.kind, worker.id, worker.name, task.id, task.title,
                        action.reason, action.finished_at,
                        action.observed_age_seconds + MAX(0, ?1 - action.finished_at)
                 {LIVE_ATTENTION_SOURCE}
                 ORDER BY action.finished_at DESC, action.id DESC LIMIT 32"
        ))?;
        statement
            .query_map([now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })?
            .map(|row| {
                let (
                    action_id,
                    kind,
                    worker_id,
                    worker_name,
                    task_id,
                    task_title,
                    reason,
                    observed_at,
                    age_seconds,
                ) = row?;
                Ok::<_, rusqlite::Error>(CoordinatorAttention {
                    action_id,
                    kind,
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    worker_name,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_title,
                    reason,
                    observed_at,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Workers with a wake queued or in flight, and since when.
    ///
    /// THE SILENCE THIS ENDS. Two tasks were routed to a sleeping Voice Bridge
    /// worker at 20:40:57. The coordinator woke it at 20:45:28. In between,
    /// four and a half minutes, nothing anywhere said a wake was coming — so
    /// the operator watched a sleeping worker, concluded "the queen didn't wake
    /// the worker", and filed that report FIFTEEN SECONDS after it woke.
    ///
    /// Queen had done nothing wrong and neither had they. The wake was sitting
    /// in `coordinator_actions` as `wake_assigned_worker` the entire time, in
    /// state queued and then running. The fact existed; nothing showed it.
    ///
    /// Reported as a FACT BESIDE the state rather than as a new attention
    /// state, the way `background_work` is. A worker that is asleep really is
    /// asleep, and rewriting what Sleeping means for every consumer to carry
    /// one more piece of news is how the Resting collapse happened.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn workers_being_woken(&self) -> Result<HashMap<WorkerId, i64>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT worker_id, MIN(created_at)
             FROM coordinator_actions
             WHERE kind = 'wake_assigned_worker' AND state IN ('queued', 'running')
             GROUP BY worker_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut waking = HashMap::new();
        for row in rows {
            let (worker_id, since) = row?;
            if let Ok(parsed) = worker_id.parse::<WorkerId>() {
                waking.insert(parsed, since);
            }
        }
        Ok(waking)
    }

    /// Claims at most one deterministic worker wake. A claimed action is never
    /// replayed after ambiguity; API startup marks it uncertain instead. The
    /// next action stays queued until a later coordination pass obtains fresh
    /// resource evidence.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn claim_coordinator_worker_wakes(
        &self,
        now: i64,
    ) -> Result<Vec<CoordinatorWorkerWake>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let night_watch = super::presence::operator_presence_from_connection(&transaction, now)?
            .mode
            == swarm_domain::PresenceMode::NightWatch;
        let approved =
            swarm_domain::ProviderKind::NIGHT_WATCH_APPROVED.map(|provider| provider.to_string());
        transaction.execute(
            "UPDATE coordinator_actions SET state = 'cancelled', updated_at = ?1
             WHERE state = 'queued' AND (
                 NOT EXISTS (
                     SELECT 1 FROM tasks task
                     WHERE task.id = coordinator_actions.task_id AND task.state = 'ready'
                       AND task.assigned_worker_id = coordinator_actions.worker_id
                 ) OR EXISTS (
                     SELECT 1 FROM worker_sessions session
                     WHERE session.worker_id = coordinator_actions.worker_id AND session.ended_at IS NULL
                 )
             )",
            [now],
        )?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT action.id, action.worker_id, action.task_id
                 FROM coordinator_actions action
                 JOIN worker_profiles worker ON worker.id = action.worker_id AND worker.archived_at IS NULL
                 JOIN tasks task ON task.id = action.task_id
                 WHERE action.kind = 'wake_assigned_worker' AND action.state = 'queued'
                   AND (NOT ?2 OR worker.provider IN (?3, ?4))
                   AND task.state = 'ready' AND task.assigned_worker_id = action.worker_id
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_sessions session
                       WHERE session.worker_id = action.worker_id AND session.ended_at IS NULL
                   )
                 ORDER BY action.created_at, action.id LIMIT ?1",
            )?;
            statement
                .query_map(
                    params![MAX_WAKE_CLAIMS, night_watch, approved[0], approved[1]],
                    |row| {
                        Ok(CoordinatorWorkerWake {
                            action_id: row.get(0)?,
                            worker_id: row
                                .get::<_, String>(1)?
                                .parse()
                                .map_err(|_| rusqlite::Error::InvalidQuery)?,
                            task_id: row
                                .get::<_, String>(2)?
                                .parse()
                                .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        for action in &candidates {
            let changed = transaction.execute(
                "UPDATE coordinator_actions SET state = 'running', attempts = 1,
                     attempted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND state = 'queued'",
                params![action.action_id, now],
            )?;
            if changed != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "coordinator wake claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Records one acknowledged worker wake.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn complete_coordinator_worker_wake(
        &self,
        action_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_coordinator_worker_wake(action_id, "completed", now)
    }

    /// Records an ambiguous worker wake without permitting replay.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn mark_coordinator_worker_wake_uncertain(
        &self,
        action_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_coordinator_worker_wake(action_id, "uncertain", now)
    }

    fn finish_coordinator_worker_wake(
        &self,
        action_id: &str,
        state: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE coordinator_actions SET state = ?2,
                 finished_at = CASE WHEN ?2 = 'completed' THEN ?3 ELSE finished_at END,
                 updated_at = ?3 WHERE id = ?1 AND state = 'running'",
            params![action_id, state, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns content-free cumulative coordinator evidence.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn coordinator_status(&self) -> Result<CoordinatorStatus, TaskStoreError> {
        let connection = self.connection()?;
        let (
            completed,
            uncertain,
            queued,
            stale_attention,
            worker_exit_attention,
            unstarted_attention,
            last_action_at,
        ): (i64, i64, i64, i64, i64, i64, Option<i64>) =
            connection.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'uncertain' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state IN ('queued','running') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'stale_owned_work_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'owned_work_worker_exited_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'assigned_ready_work_not_started_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    MAX(CASE WHEN state = 'completed' THEN finished_at ELSE updated_at END)
                 FROM coordinator_actions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )?;
        let wake_completed: i64 = connection.query_row(
            "SELECT COUNT(*) FROM coordinator_actions
             WHERE kind = 'wake_assigned_worker' AND state = 'completed'",
            [],
            |row| row.get(0),
        )?;
        Ok(CoordinatorStatus {
            completed_actions: usize::try_from(completed).unwrap_or_default(),
            queen_calls_avoided: usize::try_from(wake_completed).unwrap_or_default(),
            uncertain_actions: usize::try_from(uncertain).unwrap_or_default(),
            queued_actions: usize::try_from(queued).unwrap_or_default(),
            stale_attention_actions: usize::try_from(stale_attention).unwrap_or_default(),
            worker_exit_attention_actions: usize::try_from(worker_exit_attention)
                .unwrap_or_default(),
            unstarted_attention_actions: usize::try_from(unstarted_attention).unwrap_or_default(),
            last_action_at,
        })
    }

    /// Converts crash-interrupted worker wakes to explicit uncertainty.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_coordinator_actions(&self) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE coordinator_actions SET state = 'uncertain', updated_at = unixepoch()
             WHERE state = 'running'",
            [],
        )?)
    }
}

pub(super) fn migrate_coordinator(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind = 'wake_assigned_worker'),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA user_version = 62;",
    )
}

pub(super) fn migrate_coordinator_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let prerequisite_tables = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if prerequisite_tables != 2 {
        // Some narrow historical migration fixtures contain only the table
        // whose versioned change they exercise. They cannot contain real
        // coordinator actions, so advancing the version is both safe and
        // avoids forcing SQLite to validate unrelated absent foreign tables.
        transaction.pragma_update(None, "user_version", 63)?;
        return Ok(());
    }
    transaction.execute_batch(
        "PRAGMA legacy_alter_table = ON;
         ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v62;
         CREATE TABLE coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention')),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             session_id TEXT,
             evidence_revision INTEGER,
             observed_age_seconds INTEGER,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO coordinator_actions (
             id, idempotency_key, kind, worker_id, task_id, state, reason,
             attempts, attempted_at, finished_at, created_at, updated_at
         ) SELECT id, idempotency_key, kind, worker_id, task_id, state, reason,
                  attempts, attempted_at, finished_at, created_at, updated_at
           FROM coordinator_actions_v62;
         DROP TABLE coordinator_actions_v62;
         CREATE INDEX coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA legacy_alter_table = OFF;
         PRAGMA user_version = 63;",
    )
}

pub(super) fn migrate_coordinator_worker_exit_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let prerequisite_tables = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles', 'coordinator_actions')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if prerequisite_tables != 3 {
        transaction.pragma_update(None, "user_version", 64)?;
        return Ok(());
    }
    transaction.execute_batch(
        "DROP INDEX IF EXISTS coordinator_actions_queue;
         PRAGMA legacy_alter_table = ON;
         ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v63;
         CREATE TABLE coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention')),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             session_id TEXT,
             evidence_revision INTEGER,
             observed_age_seconds INTEGER,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO coordinator_actions (
             id, idempotency_key, kind, worker_id, task_id, session_id,
             evidence_revision, observed_age_seconds, state, reason, attempts,
             attempted_at, finished_at, created_at, updated_at
         ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason, attempts,
                  attempted_at, finished_at, created_at, updated_at
           FROM coordinator_actions_v63;
         DROP TABLE coordinator_actions_v63;
         CREATE INDEX coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA legacy_alter_table = OFF;
         PRAGMA user_version = 64;",
    )
}

pub(super) fn migrate_coordinator_unstarted_work_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let prerequisite_tables = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles', 'coordinator_actions')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if prerequisite_tables != 3 {
        transaction.pragma_update(None, "user_version", 65)?;
        return Ok(());
    }
    transaction.execute_batch(
        "DROP INDEX IF EXISTS coordinator_actions_queue;
         PRAGMA legacy_alter_table = ON;
         ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v64;
         CREATE TABLE coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention')),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             session_id TEXT,
             evidence_revision INTEGER,
             observed_age_seconds INTEGER,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO coordinator_actions (
             id, idempotency_key, kind, worker_id, task_id, session_id,
             evidence_revision, observed_age_seconds, state, reason, attempts,
             attempted_at, finished_at, created_at, updated_at
         ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason, attempts,
                  attempted_at, finished_at, created_at, updated_at
           FROM coordinator_actions_v64;
         DROP TABLE coordinator_actions_v64;
         CREATE INDEX coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA legacy_alter_table = OFF;
         PRAGMA user_version = 65;",
    )
}

/// Admits the one kind of attention a worker can raise itself.
///
/// `kind` carries a CHECK, and the writer uses INSERT OR IGNORE — which
/// suppresses a constraint violation as readily as a duplicate. A new kind
/// therefore fails silently and completely until the constraint admits it,
/// which is why this migration exists rather than the insert simply working.
///
/// # Errors
/// Returns an error when the step cannot be applied.
/// Admits the kind an overdue decision will be recorded under, before anything
/// writes one.
///
/// Item 53's hazard, applied deliberately this time rather than discovered:
/// `coordinator_actions.kind` carries a CHECK and the detectors write with a
/// conflict clause, so a kind the schema does not admit fails in a way that is
/// easy to read as "nothing to do". Widening first means the detector that
/// follows cannot be silently inert.
///
/// Nothing writes this kind yet. The detector needs Queen's inbox to tolerate a
/// row whose worker has no live session — a worker that files a decision and
/// stops is exactly the case — and that is a change to a query which has
/// already been the source of one three-copies bug. It is worth doing on its
/// own rather than inside a release cut to test an upgrade.
///
/// # Errors
/// Returns an error when the step cannot be applied.
pub(super) fn migrate_decision_deadline_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%decision_deadline_passed_attention%')",
        [],
        |row| row.get(0),
    )?;
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%worker_filed_draft_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v84;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason, attempts,
                 attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason, attempts,
                      attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v84;
             DROP TABLE coordinator_actions_v84;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    Ok(())
}

pub(super) fn migrate_worker_filed_draft_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%worker_filed_draft_attention%')",
        [],
        |row| row.get(0),
    )?;
    // The steps that built this table's current shape bail when their own
    // prerequisites are absent, so a database can arrive here still carrying an
    // older one. Rebuild only what is actually the shape this widens; anything
    // else is left alone rather than rewritten from a guess.
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%assigned_ready_work_not_started_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v80;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason, attempts,
                 attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason, attempts,
                      attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v80;
             DROP TABLE coordinator_actions_v80;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(None, "user_version", WORKER_FILED_DRAFT_SCHEMA_VERSION)
}

/// Tells "the worker has the brief and is idle" apart from "the worker was
/// never given the brief".
///
/// Those look identical from outside — no commits, no files touched, a resting
/// worker on Active work — and `stale_owned_work_attention` reported both as
/// the first, which points the reader at a worker that is innocent. Measured
/// 2026-08-19: a high-priority brief sat undelivered for 27 hours while every
/// board read the task as work in progress.
///
/// A separate kind rather than a different wording, because the two need
/// different actions: one is steer the worker, the other is redeliver the
/// brief.
pub(super) fn migrate_undelivered_brief_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%owned_work_never_briefed_attention%')",
        [],
        |row| row.get(0),
    )?;
    // As with the step before it: a database can arrive here still carrying an
    // older shape, so widen only the shape this actually widens.
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%worker_filed_draft_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v88;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason, attempts,
                 attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason, attempts,
                      attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v88;
             DROP TABLE coordinator_actions_v88;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::UNDELIVERED_BRIEF_ATTENTION_SCHEMA_VERSION,
    )
}

/// Adds the kind that says finished work is waiting on evidence nobody gave it.
///
/// Review was the one task state no attention kind covered. Work that was done
/// and work that was abandoned looked identical on the board, and the only way
/// to tell them apart was reading the handoff prose — which is how three tasks
/// sat stranded in a day.
/// Work that has sat BLOCKED long enough that nobody is coming.
///
/// The operator: "we have workers with blocked tasks and the queen never
/// changes anything". Measured before building: one task blocked 168.6 hours —
/// seven days — and two more past a day. Nothing on the board distinguished
/// them from a block ten minutes old, because a blocked task is a legitimate
/// resting state and the only signal was the state itself.
///
/// This is a CLOCK rather than a channel, and deliberately: coordinator
/// attention already exists and already reaches Queen, so a block older than a
/// threshold with no Queen action on it is a computable fact needing no new
/// route between workers.
///
/// It does not let anyone unblock anything. Queen remains the only actor that
/// moves a task out of Blocked, which is the line the operator drew: "we don't
/// want to lose our design of the queen being an arbitrator."
/// Adds the settled-but-unclosed attention kind.
///
/// Same rebuild the previous kinds used, because `SQLite` cannot widen a CHECK
/// in place. `coordinator_actions` is safe to rebuild: its foreign keys point
/// outward at tasks and workers and nothing holds a key into it.
pub(super) fn migrate_evidenced_work_not_closed_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%evidenced_work_not_closed_attention%')",
        [],
        |row| row.get(0),
    )?;
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%blocked_work_unattended_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v112;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention','blocked_work_unattended_attention','evidenced_work_not_closed_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason, attempts,
                 attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason, attempts,
                      attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v112;
             DROP TABLE coordinator_actions_v112;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::EVIDENCED_WORK_NOT_CLOSED_SCHEMA_VERSION,
    )
}

pub(super) fn migrate_unattended_block_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%blocked_work_unattended_attention%')",
        [],
        |row| row.get(0),
    )?;
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%reviewed_work_without_evidence_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v99;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention','blocked_work_unattended_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason, attempts,
                 attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason, attempts,
                      attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v99;
             DROP TABLE coordinator_actions_v99;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(None, "user_version", crate::UNATTENDED_BLOCK_SCHEMA_VERSION)
}

pub(super) fn migrate_reviewed_work_evidence_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%reviewed_work_without_evidence_attention%')",
        [],
        |row| row.get(0),
    )?;
    // Only widen the shape this actually widens: a database can arrive here
    // carrying an older one.
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%owned_work_never_briefed_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v89;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason, attempts,
                 attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason, attempts,
                      attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v89;
             DROP TABLE coordinator_actions_v89;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::REVIEWED_WORK_EVIDENCE_ATTENTION_SCHEMA_VERSION,
    )
}

#[cfg(test)]
mod unattended_block_tests {
    use crate::TaskStore;
    use swarm_domain::{ProviderKind, TaskState, WorkerSessionId};

    /// A block that has waited surfaces, and its age is measured from when it
    /// was BLOCKED.
    ///
    /// The operator: "we have workers with blocked tasks and the queen never
    /// changes anything." Measured against the live database first: one task
    /// blocked 168.6 hours, two more past a day. Nothing distinguished them from
    /// a ten-minute block, because a blocked task is a legitimate resting state.
    ///
    /// THE MEASUREMENT IS THE FRAGILE PART. `updated_at` reported 0.6 hours for
    /// every one of those tasks — a sweep had touched them — so a query written
    /// the obvious way reports a healthy board while a week-old block sits in it.
    ///
    /// Queen moving the task is what clears this, which is the design the
    /// operator asked to keep: she remains the only actor that takes work out of
    /// Blocked, and nothing here changes that.
    /// Approved evidence with no closure, which nothing watched for nine hours.
    ///
    /// Found on the real board as one row in 481: state `review`,
    /// `closed_on_evidence` true, claimed 02:25 and approved by Queen 04:25,
    /// still open at 13:27. It was invisible from both directions — the
    /// evidence detector skips it BECAUSE the exemption is approved, and
    /// nothing counts it as done because it is not completed.
    ///
    /// The negative half is the point: this must stay quiet for work that is
    /// merely claimed (that is the other detector's job, and firing here too
    /// would double-report every honest handoff) and for work that closed.
    #[test]
    fn evidence_approved_but_never_closed_is_surfaced_and_clears_when_queen_closes_it() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Public Web",
                ProviderKind::ClaudeCode,
                "/workspace/public-web",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("A spike with nothing to ship", "/workspace/public-web")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(
                task.id,
                worker.id,
                &swarm_domain::TaskActivityActor::operator(),
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();

        let now: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .unwrap();
        let an_hour = 3_600;

        store
            .claim_completion_exemption(task.id, "An investigation with no code.", None, now)
            .unwrap();

        // A CLAIM ALONE MUST SAY NOTHING HERE. Getting it approved is the other
        // detector's question, and answering it twice would report every honest
        // handoff as a problem the moment it was filed.
        assert!(
            store
                .evidenced_work_not_closed_candidates(now + an_hour, 60)
                .unwrap()
                .is_empty(),
            "an unapproved claim is not settled evidence"
        );

        store
            .approve_completion_exemption(task.id, "queen", "Read the handoff.", now)
            .unwrap();

        // Still fresh: approving and closing are two acts and the second is
        // allowed to take a moment.
        assert!(
            store
                .evidenced_work_not_closed_candidates(now, an_hour)
                .unwrap()
                .is_empty(),
            "an approval seconds old is not yet an abandoned task"
        );

        let candidates = store
            .evidenced_work_not_closed_candidates(now + an_hour + 60, an_hour)
            .unwrap();
        assert_eq!(
            candidates.len(),
            1,
            "settled evidence that never closed has to reach someone"
        );
        assert_eq!(candidates[0].task_id, task.id);

        // AND IT CLEARS BY THE ONLY ACT THAT ANSWERS IT.
        store
            .transition_task(task.id, TaskState::Completed)
            .unwrap();
        assert!(
            store
                .evidenced_work_not_closed_candidates(now + an_hour + 120, an_hour)
                .unwrap()
                .is_empty(),
            "closing the task is what settles this, and nothing else has to"
        );
    }

    /// The check has to be silent on a board with nothing wrong with it.
    ///
    /// A detector that only ever fires is indistinguishable from a working one
    /// when the board it is first run against genuinely has an anomaly in it —
    /// which is exactly how this one was found.
    #[test]
    fn a_board_with_no_stranded_evidence_surfaces_nothing() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Public Web",
                ProviderKind::ClaudeCode,
                "/workspace/public-web",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let now: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .unwrap();
        let an_hour = 3_600;

        let make = |title: &str| {
            let task = store.create_task(title, "/workspace/public-web").unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store
                .assign_task_to_worker_as(
                    task.id,
                    worker.id,
                    &swarm_domain::TaskActivityActor::operator(),
                )
                .unwrap();
            store.transition_task(task.id, TaskState::Active).unwrap();
            task
        };

        // Reviewed with no evidence at all: the OTHER detector's business.
        let bare = make("Reviewed, nothing claimed");
        store.transition_task(bare.id, TaskState::Review).unwrap();

        // Properly closed on an approved exemption.
        let closed = make("Closed on an approved exemption");
        store.transition_task(closed.id, TaskState::Review).unwrap();
        store
            .claim_completion_exemption(closed.id, "A document.", None, now)
            .unwrap();
        store
            .approve_completion_exemption(closed.id, "queen", "Read the handoff.", now)
            .unwrap();
        store
            .transition_task(closed.id, TaskState::Completed)
            .unwrap();

        // Still being worked on.
        let active = make("Still in progress");

        assert!(
            store
                .evidenced_work_not_closed_candidates(now + an_hour + 60, an_hour)
                .unwrap()
                .is_empty(),
            "nothing on this board is stranded, so nothing should be reported"
        );
        let _ = (bare, closed, active);
    }

    #[test]
    fn blocked_work_nobody_returned_to_is_surfaced_and_clears_when_queen_moves_it() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Sculpt Studio",
                ProviderKind::ClaudeCode,
                "/workspace/sculpt",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task(
                "iOS: the bottom tab bar detaches mid-page",
                "/workspace/sculpt",
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(
                task.id,
                worker.id,
                &swarm_domain::TaskActivityActor::operator(),
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Blocked).unwrap();

        // The task's own updated_at is the clock this test needs to be relative
        // to, and reading it from the row avoids depending on wall time.
        let now: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .unwrap();
        let a_week = 7 * 24 * 3_600;

        // Fresh: nothing to say.
        assert!(
            store
                .unattended_block_candidates(now, a_week)
                .unwrap()
                .is_empty(),
            "a block that just happened is not a problem"
        );

        // THE SWEEP. This is what makes the difference between the two ways of
        // measuring, and without it this test passes either way — it did, until
        // an ablation showed the assertion below could not tell them apart.
        //
        // On the live database every blocked task reported 0.6 hours because
        // something had touched updated_at, while the oldest had been blocked
        // for a week. Reproducing that here is the only way this test covers the
        // thing it claims to.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
                rusqlite::params![task.id.to_string(), now + a_week],
            )
            .unwrap();

        // Aged past the threshold, timed from the transition.
        let candidates = store
            .unattended_block_candidates(now + a_week + 60, 60)
            .unwrap();
        assert_eq!(candidates.len(), 1, "a block nobody returned to surfaces");
        assert_eq!(candidates[0].task_id, task.id);
        assert!(
            candidates[0].age_seconds >= a_week,
            "age is measured from the transition and NOT from updated_at, which a \
             sweep has just moved to a week later: {}",
            candidates[0].age_seconds
        );

        assert!(
            store
                .record_unattended_block_attention(&candidates[0], now + a_week + 60, 60)
                .unwrap()
        );
        assert_eq!(
            store
                .current_coordinator_attention(now + a_week + 60)
                .unwrap()
                .len(),
            1,
            "and it reaches the place Queen already looks"
        );

        // Queen moves it. Nothing deletes the row; its reason has passed.
        store.transition_task(task.id, TaskState::Ready).unwrap();
        assert!(
            store
                .current_coordinator_attention(now + a_week + 120)
                .unwrap()
                .is_empty(),
            "acting on the block is what clears the flag"
        );
    }
}

#[cfg(test)]
mod reviewed_work_tests {
    use crate::TaskStore;
    use swarm_domain::{ProviderKind, TaskState, WorkerSessionId};

    /// Review was the one task state no attention kind covered, so finished
    /// work and abandoned work looked identical on the board. Three tasks sat
    /// stranded in a day because the only way to tell them apart was reading
    /// the handoff prose.
    /// The claim is the ASK, not the answer, and this test used to say the
    /// opposite.
    ///
    /// It asserted "the claim clears it without anything having to delete the
    /// row", which is precisely the defect: a worker calling
    /// `swarm_record_no_deployment` switched off the detector whose entire job
    /// is to get that claim approved. A worker cannot approve its own
    /// exemption, so the one signal that a person still owed something was
    /// silenced by the act of asking. Queen approved 25 of 33 claims on
    /// 2026-08-31 and every approval landed within eight minutes -- she is not
    /// slow, she is only ever told once, while she still happens to be in the
    /// conversation. The eight she was not in the conversation for reached the
    /// operator's card instead, which is not a surface that settles anything.
    ///
    /// A green test asserting the wrong behaviour is why this survived. So the
    /// assertion is inverted rather than deleted.
    #[test]
    fn finished_work_with_no_evidence_is_surfaced_and_an_unapproved_claim_does_not_clear_it() {
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
            .create_task("Read-only investigation", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();

        let now = i64::MAX / 4;
        let grace = 15 * 60;
        let fresh = store
            .reviewed_work_without_evidence_candidates(0, grace)
            .unwrap();
        assert!(fresh.is_empty(), "work just reported is not yet stranded");

        let candidates = store
            .reviewed_work_without_evidence_candidates(now, grace)
            .unwrap();
        assert_eq!(candidates.len(), 1, "no evidence of either kind");
        assert_eq!(candidates[0].task_id, task.id);
        assert!(
            store
                .record_reviewed_work_without_evidence_attention(&candidates[0], now, grace)
                .unwrap()
        );
        assert!(
            store
                .current_coordinator_attention(now)
                .unwrap()
                .iter()
                .any(|attention| attention.kind == "reviewed_work_without_evidence_attention"),
            "Queen can see it"
        );

        // The worker asks. Asking is not being answered, so Queen keeps seeing
        // it -- this is the assertion whose inverse let eight claims go
        // invisible on 2026-08-31.
        store
            .claim_completion_exemption(task.id, "Read-only investigation", Some(worker.id), now)
            .unwrap();
        assert!(
            store
                .current_coordinator_attention(now)
                .unwrap()
                .iter()
                .any(|attention| attention.kind == "reviewed_work_without_evidence_attention"),
            "an unapproved claim is the request for judgment, so it must not silence the request"
        );

        // Queen answers, and only then does it clear -- still without anything
        // having to be dismissed by hand.
        store
            .approve_completion_exemption(task.id, "queen", "Read the handoff.", now)
            .unwrap();
        assert!(
            !store
                .current_coordinator_attention(now)
                .unwrap()
                .iter()
                .any(|attention| attention.kind == "reviewed_work_without_evidence_attention"),
            "the approval clears it without anything having to delete the row"
        );
    }

    /// The candidate query and the live view must agree about what counts.
    ///
    /// They are separate SQL in separate places -- `LIVE_ATTENTION_SOURCE`, the
    /// candidate selection, and the re-check inside
    /// `record_reviewed_work_without_evidence_attention` -- and all three
    /// carried the same wrong guard. Fixing two of three would leave the
    /// detector raising work it then refused to record, so the agreement is
    /// asserted rather than assumed.
    #[test]
    fn an_unapproved_claim_does_not_stop_the_work_being_a_candidate() {
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
            .create_task("Claimed but unapproved", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        let now = i64::MAX / 4;
        let grace = 15 * 60;
        store
            .claim_completion_exemption(task.id, "nothing to deploy", Some(worker.id), now)
            .unwrap();

        let candidates = store
            .reviewed_work_without_evidence_candidates(now, grace)
            .unwrap();
        assert_eq!(
            candidates.len(),
            1,
            "a claim nobody approved still needs her"
        );
        assert_eq!(candidates[0].task_id, task.id);
        assert!(
            store
                .record_reviewed_work_without_evidence_attention(&candidates[0], now, grace)
                .unwrap(),
            "the re-check inside recording must agree with the selection"
        );

        store
            .approve_completion_exemption(task.id, "queen", "Read the handoff.", now)
            .unwrap();
        assert!(
            store
                .reviewed_work_without_evidence_candidates(now, grace)
                .unwrap()
                .is_empty(),
            "approved evidence is evidence"
        );
    }

    /// WITHDRAWING AN APPROVED CLAIM PUTS THE WORK BACK IN FRONT OF HER, and
    /// that is the whole reason withdrawal has to exist rather than being a
    /// tidier way to write a note.
    ///
    /// Approving is the ONE act that takes a task off this detector. So a claim
    /// that was approved and later found false is the only genuinely invisible
    /// case on this board: the task looks settled, nothing is watching it, and
    /// the record says work shipped nowhere. One such claim was approved at
    /// 04:25 on work whose PR was still open.
    ///
    /// The second half is the control, and it is the half that would have caught
    /// me implementing this as "any exemption row stops counting". A live
    /// approved claim on a second task must be untouched by the same query.
    #[test]
    fn a_withdrawn_approval_makes_the_work_visible_again_and_a_live_one_does_not() {
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
        let now = i64::MAX / 4;
        let grace = 15 * 60;

        let reviewed = |title: &str| {
            let task = store.create_task(title, "/workspace/petal").unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store.assign_task(task.id, session).unwrap();
            store.transition_task(task.id, TaskState::Active).unwrap();
            store.transition_task(task.id, TaskState::Review).unwrap();
            store
                .claim_completion_exemption(task.id, "nothing to deploy", Some(worker.id), now)
                .unwrap();
            store
                .approve_completion_exemption(task.id, "queen", "Read the handoff.", now)
                .unwrap();
            task
        };
        let withdrawn = reviewed("Approved, then found false");
        let _live = reviewed("Approved and still true");

        assert!(
            store
                .reviewed_work_without_evidence_candidates(now, grace)
                .unwrap()
                .is_empty(),
            "both are approved, so neither is stranded yet"
        );

        store
            .withdraw_completion_exemption(withdrawn.id, "queen", now)
            .unwrap();

        let candidates = store
            .reviewed_work_without_evidence_candidates(now, grace)
            .unwrap();
        assert_eq!(
            candidates.iter().map(|row| row.task_id).collect::<Vec<_>>(),
            vec![withdrawn.id],
            "the withdrawn one comes back and the live one stays settled"
        );
        assert!(
            store
                .record_reviewed_work_without_evidence_attention(&candidates[0], now, grace)
                .unwrap(),
            "the re-check inside recording must agree with the selection"
        );
    }

    /// A task that shipped is not stranded, so it is never raised.
    #[test]
    fn recorded_deployment_is_not_stranded_work() {
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
            .create_task("Shipped work", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        let now = i64::MAX / 4;
        store
            .record_task_deployment(task.id, "production", "abc123", now)
            .unwrap();

        assert!(
            store
                .reviewed_work_without_evidence_candidates(now, 15 * 60)
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkerId, WorkerSessionId};
    use crate::TaskStore;

    /// A decision that has gone past its deadline, with everything the inbox
    /// joins: a worker that has held a session, and a task to hang it off.
    fn overdue_decision(
        store: &TaskStore,
        deadline_ago: i64,
    ) -> (WorkerId, swarm_domain::DecisionRequestId) {
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
            .create_task("Answer this", "/workspace/petal")
            .unwrap();
        let actions = vec!["continue".into(), "stop".into()];
        let decision = store
            .create_decision_request(&crate::decisions::NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: Some(task.id),
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Which environment?",
                summary: "Staging or production for the first run.",
                reason: "Both are plausible and the choice is not reversible.",
                risk: "Deploying to the wrong one is visible to customers.",
                evidence: "Both environments are healthy.",
                suggested_action: "continue",
                allowed_actions: &actions,
                questions: &[],
                // A deadline in the past is refused at creation, so it is set
                // in the future and then aged below — which is also what
                // actually happens to one.
                deadline: Some(unix_now(store) + 3_600),
                requested_command: None,
            })
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE decision_requests SET deadline = unixepoch() - ?2 WHERE id = ?1",
                rusqlite::params![decision.id.to_string(), deadline_ago],
            )
            .unwrap();
        (worker.id, decision.id)
    }

    fn unix_now(store: &TaskStore) -> i64 {
        store
            .connection()
            .unwrap()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .unwrap()
    }

    fn attention_kinds(store: &TaskStore) -> Vec<String> {
        store
            .current_coordinator_attention(0)
            .unwrap()
            .into_iter()
            .map(|attention| attention.kind)
            .collect()
    }

    /// The operator's ruling: "queen should make this a needs you item." Before
    /// this, a deadline was recorded, shown on the roster as overdue, and acted
    /// on by nobody.
    #[test]
    fn a_decision_past_its_deadline_reaches_queens_inbox() {
        let store = TaskStore::in_memory().unwrap();
        let (_, decision_id) = overdue_decision(&store, 600);

        let candidates = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].decision_id, decision_id.to_string());
        assert!(candidates[0].overdue_seconds >= 600);

        assert!(
            store
                .record_overdue_decision_attention(&candidates[0])
                .unwrap()
        );
        assert!(attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned()));

        // Raising it twice for one decision would make Queen review the same
        // fact repeatedly.
        assert!(
            !store
                .record_overdue_decision_attention(&candidates[0])
                .unwrap()
        );
        assert_eq!(
            attention_kinds(&store)
                .iter()
                .filter(|kind| *kind == "decision_deadline_passed_attention")
                .count(),
            1
        );
    }

    /// Answering it is what clears it. Nothing deletes the row.
    #[test]
    fn answering_the_decision_clears_the_attention() {
        let store = TaskStore::in_memory().unwrap();
        let (_, decision_id) = overdue_decision(&store, 600);
        let candidates = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        store
            .record_overdue_decision_attention(&candidates[0])
            .unwrap();
        assert!(attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned()));

        // Answered the way an operator answers it, not by editing the row.
        store
            .resolve_decision_request(decision_id, "continue", "Staging first.", "operator")
            .unwrap();

        assert!(
            !attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned())
        );
        assert!(
            store
                .overdue_decision_candidates(unix_now(&store))
                .unwrap()
                .is_empty()
        );
    }

    /// The inbox only counts an observation whose evidence still matches the
    /// task it was made against, and the idempotency key stops a second row
    /// being raised — so without re-stamping, a task that moved on would
    /// silence an attention whose decision is still waiting.
    #[test]
    fn a_task_moving_on_does_not_silence_a_decision_still_waiting() {
        let store = TaskStore::in_memory().unwrap();
        overdue_decision(&store, 600);
        let candidates = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        store
            .record_overdue_decision_attention(&candidates[0])
            .unwrap();

        // The task moves on. Advanced explicitly rather than by transitioning
        // it: updated_at is a whole-second stamp, so a real edit inside the
        // same second as the observation leaves the revision unchanged and the
        // test would prove nothing.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = updated_at + 1 WHERE id = ?1",
                [candidates[0].task_id.to_string()],
            )
            .unwrap();
        assert!(
            !attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned()),
            "the observation should have gone stale with the task"
        );

        // The next pass re-stamps it rather than raising a second one.
        let again = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        assert!(store.record_overdue_decision_attention(&again[0]).unwrap());
        assert_eq!(
            attention_kinds(&store)
                .iter()
                .filter(|kind| *kind == "decision_deadline_passed_attention")
                .count(),
            1
        );
    }

    /// The grace period is what stops a prompt answered in ten seconds from
    /// becoming an item in the operator's queue.
    #[test]
    fn a_refusal_is_silent_until_it_has_lasted_long_enough_to_matter() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "Queen cannot review while her terminal has an unanswered prompt",
                1_000,
            )
            .unwrap();

        assert!(
            store
                .standing_coordinator_refusals(1_030, 120)
                .unwrap()
                .is_empty()
        );
        // Still being refused, as a real hold is — every retry re-observes it.
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "Queen cannot review while her terminal has an unanswered prompt",
                1_190,
            )
            .unwrap();
        let standing = store.standing_coordinator_refusals(1_200, 120).unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].subject, "queen-review");
    }

    /// The operator's report: "I see this, but it isn't true."
    ///
    /// Two task briefs had been refused four times and once, half an hour
    /// earlier, and had not been retried since — the dispatches had moved on
    /// without ever succeeding, so nothing cleared them. The queue was still
    /// announcing them as work waiting at a prompt, with a retry count frozen
    /// where it had stopped.
    ///
    /// Missing new observations cannot establish recovery. Preserve the recorded
    /// prompt hold until explicit resolution or ended-session evidence.
    #[test]
    fn an_unobserved_prompt_hold_is_not_assumed_resolved_by_age() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "task-brief:abandoned",
                None,
                None,
                "a briefing is waiting",
                1_000,
            )
            .unwrap();

        // Still being retried: still standing.
        assert_eq!(
            store
                .standing_coordinator_refusals(1_150, 120)
                .unwrap()
                .len(),
            1
        );

        // Nothing has touched it since. Current state is unknown, not resolved.
        let stale = 1_000 + TaskStore::STALE_REFUSAL_SECONDS + 1;
        let retained = store.standing_coordinator_refusals(stale, 120).unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].last_observed_at, 1_000);
        store
            .clear_coordinator_refusal(REFUSAL_DELIVERY_HELD, "task-brief:abandoned", stale)
            .unwrap();
        assert!(
            store
                .standing_coordinator_refusals(stale, 120)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn refusal_projection_overflow_is_unavailable_not_a_partial_all_clear() {
        let store = TaskStore::in_memory().unwrap();
        for subject in 0..257 {
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD,
                    &format!("task-brief:{subject}"),
                    None,
                    None,
                    "held",
                    1_000,
                )
                .unwrap();
        }
        assert!(store.standing_coordinator_refusals(10_000, 120).is_err());
        store
            .clear_coordinator_refusal(REFUSAL_DELIVERY_HELD, "task-brief:0", 10_000)
            .unwrap();
        assert_eq!(
            store
                .standing_coordinator_refusals(10_000, 120)
                .unwrap()
                .len(),
            256
        );
    }

    /// A hold that is still happening keeps being reported, however long it has
    /// lasted. The window is about whether anyone is still retrying, not about
    /// how patient the operator should be.
    #[test]
    fn a_hold_that_is_still_being_retried_keeps_standing_however_old() {
        let store = TaskStore::in_memory().unwrap();
        for minute in 0..30 {
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD,
                    "queen-review",
                    None,
                    None,
                    "Queen's review is waiting",
                    1_000 + minute * 60,
                )
                .unwrap();
        }

        let standing = store
            .standing_coordinator_refusals(1_000 + 29 * 60, 120)
            .unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].observations, 30);
    }

    /// One row per stuck thing, not one per attempt. The measured case retried
    /// 1503 times in twenty-four hours; that is a count, not 1503 entries.
    #[test]
    fn repeated_refusals_are_one_row_with_a_count() {
        let store = TaskStore::in_memory().unwrap();
        for at in [1_000, 1_030, 1_060] {
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD,
                    "queen-review",
                    None,
                    None,
                    "held",
                    at,
                )
                .unwrap();
        }

        let standing = store.standing_coordinator_refusals(1_200, 120).unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].observations, 3);
        // The age is measured from when it started, not from the last check.
        assert_eq!(standing[0].first_observed_at, 1_000);
    }

    #[test]
    fn refusal_session_change_starts_a_new_occurrence() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Poppy",
                ProviderKind::ClaudeCode,
                "/workspace/poppy",
                false,
                1,
            )
            .unwrap();
        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        for at in [1_000, 1_030] {
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD,
                    "brief",
                    Some(worker.id),
                    Some(first),
                    "held",
                    at,
                )
                .unwrap();
        }
        store.release_worker_session(first).unwrap();
        assert!(
            store
                .standing_coordinator_refusals(1_035, 0)
                .unwrap()
                .is_empty()
        );
        let second = WorkerSessionId::new();
        store.bind_worker_session(worker.id, second).unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "brief",
                Some(worker.id),
                Some(second),
                "new session",
                1_040,
            )
            .unwrap();
        let standing = store.standing_coordinator_refusals(1_040, 0).unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].first_observed_at, 1_040);
        assert_eq!(standing[0].observations, 1);
    }

    #[test]
    fn refusal_prompt_reason_replaces_only_its_own_delivery_condition() {
        let store = TaskStore::in_memory().unwrap();
        for (kind, subject) in [
            (REFUSAL_DELIVERY_HELD, "queen-review"),
            (REFUSAL_DELIVERY_HELD, "another-brief"),
            (REFUSAL_WAKE_UNCERTAIN, "queen-review"),
        ] {
            store
                .record_coordinator_refusal(kind, subject, None, None, "held", 1_000)
                .unwrap();
        }
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
                "queen-review",
                None,
                None,
                "unsent",
                1_010,
            )
            .unwrap();
        let standing = store.standing_coordinator_refusals(1_010, 0).unwrap();
        assert_eq!(standing.len(), 3);
        assert!(
            !standing
                .iter()
                .any(|r| r.subject == "queen-review" && r.kind == REFUSAL_DELIVERY_HELD)
        );
        assert!(standing.iter().any(|r| r.subject == "another-brief"));
        assert!(standing.iter().any(|r| r.kind == REFUSAL_WAKE_UNCERTAIN));
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "question again",
                1_020,
            )
            .unwrap();
        let again = store.standing_coordinator_refusals(1_020, 0).unwrap();
        let question = again
            .iter()
            .find(|r| r.subject == "queen-review" && r.kind == REFUSAL_DELIVERY_HELD)
            .unwrap();
        assert_eq!(question.first_observed_at, 1_020);
        assert_eq!(question.observations, 1);
        assert!(
            !again
                .iter()
                .any(|r| r.kind == REFUSAL_DELIVERY_HELD_UNSENT_TEXT)
        );
    }

    #[test]
    fn refusal_replacement_failure_preserves_the_previous_observation() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "question",
                1_000,
            )
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER reject_refusal BEFORE INSERT ON coordinator_refusals
             WHEN NEW.kind = 'delivery_held_unsent_text'
             BEGIN SELECT RAISE(ABORT, 'injected refusal write failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
                    "queen-review",
                    None,
                    None,
                    "unsent",
                    1_010
                )
                .is_err()
        );
        let standing = store.standing_coordinator_refusals(1_010, 0).unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].kind, REFUSAL_DELIVERY_HELD);
        assert_eq!(standing[0].observations, 1);
    }

    /// Answering the prompt clears it, and nothing has to delete the row.
    #[test]
    fn clearing_a_refusal_takes_it_out_of_the_queue() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "held",
                1_000,
            )
            .unwrap();
        assert!(
            store
                .clear_coordinator_refusal(REFUSAL_DELIVERY_HELD, "queen-review", 1_100)
                .unwrap()
        );
        assert!(
            store
                .standing_coordinator_refusals(1_400, 120)
                .unwrap()
                .is_empty()
        );

        // And a recurrence is a new occurrence, not a continuation of the old
        // one — otherwise a hold that cleared last week would appear to have
        // been standing ever since.
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "held",
                2_000,
            )
            .unwrap();
        // Recurring, so still being observed when the queue is read.
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "held",
                2_190,
            )
            .unwrap();
        let again = store.standing_coordinator_refusals(2_200, 120).unwrap();
        assert_eq!(again[0].first_observed_at, 2_000);
        assert_eq!(again[0].observations, 2);
    }

    /// A deadline that has not passed is not overdue, whatever else is true.
    #[test]
    fn a_decision_inside_its_deadline_is_left_alone() {
        let store = TaskStore::in_memory().unwrap();
        overdue_decision(&store, -3_600);
        assert!(
            store
                .overdue_decision_candidates(unix_now(&store))
                .unwrap()
                .is_empty()
        );
    }

    /// Item 53's hazard, avoided on purpose this time. The kind CHECK and the
    /// conflict clause together turn an unadmitted kind into a write that does
    /// nothing and reports nothing, so the schema admits the kind before any
    /// detector writes it.
    #[test]
    fn the_overdue_decision_kind_is_admitted_before_anything_writes_one() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();

        let admitted: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%decision_deadline_passed_attention%')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            admitted,
            "the kind is not in the CHECK, so writing it would be silently inert"
        );

        // And a kind nobody admitted still fails loudly rather than quietly.
        let rejected = connection.execute(
            "INSERT INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, state, reason)
             VALUES ('x', 'x', 'not_a_kind', 'w', 't', 'completed', 'r')",
            [],
        );
        assert!(rejected.is_err(), "an unadmitted kind was accepted");
    }
    use super::*;
    use crate::decisions::NewDecisionRequest;
    use swarm_domain::{
        DecisionRequestKind, DecisionUrgency, ProviderKind, TaskActivityActor, TaskPriority,
        TaskState,
    };

    fn active_owned_work(
        store: &TaskStore,
        worker_name: &str,
        updated_at: i64,
    ) -> (WorkerId, WorkerSessionId, TaskId) {
        let worker = store
            .create_worker(
                worker_name,
                ProviderKind::ClaudeCode,
                &format!("/workspace/{worker_name}"),
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task(
                "Keep the release moving",
                &format!("/workspace/{worker_name}"),
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
                params![task.id.to_string(), updated_at],
            )
            .unwrap();
        (worker.id, session, task.id)
    }

    fn assert_v64_to_v65_preserves_action(
        transaction: &rusqlite::Transaction<'_>,
        task_id: TaskId,
    ) {
        migrate_coordinator_unstarted_work_attention(transaction).unwrap();
        assert_eq!(
            transaction
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            65
        );
        let (kind, table_sql): (String, String) = (
            transaction
                .query_row(
                    "SELECT kind FROM coordinator_actions WHERE task_id = ?1",
                    [task_id.to_string()],
                    |row| row.get(0),
                )
                .unwrap(),
            transaction
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_actions'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
        );
        assert_eq!(kind, "wake_assigned_worker");
        assert!(table_sql.contains("assigned_ready_work_not_started_attention"));
    }

    #[test]
    fn queen_assignment_queues_one_durable_sleeping_worker_wake() {
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
        let task = store
            .create_task_with_details(
                "Polish the task board",
                "Keep it dense and readable.",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();

        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let actions = store.claim_coordinator_worker_wakes(100).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].worker_id, worker.id);
        assert_eq!(actions[0].task_id, task.id);
        assert!(
            store
                .complete_coordinator_worker_wake(&actions[0].action_id, 101)
                .unwrap()
        );
        let status = store.coordinator_status().unwrap();
        assert_eq!(status.completed_actions, 1);
        assert_eq!(status.queen_calls_avoided, 1);
    }

    #[test]
    fn night_watch_defers_experimental_wakes_and_resumes_without_losing_work() {
        for provider in [
            ProviderKind::Gemini,
            ProviderKind::Grok,
            ProviderKind::OpenCode,
        ] {
            let store = TaskStore::in_memory().unwrap();
            let queen = store.ensure_queen("/workspace/queen").unwrap();
            let mut workers = Vec::new();
            for (name, kind, workspace) in [
                ("Experimental", provider, "/workspace/experimental"),
                ("Approved", ProviderKind::ClaudeCode, "/workspace/approved"),
            ] {
                let worker = store
                    .create_worker(name, kind, workspace, false, 1)
                    .unwrap();
                let task = store.create_task(name, workspace).unwrap();
                store
                    .transition_task(task.id, swarm_domain::TaskState::Ready)
                    .unwrap();
                store
                    .assign_task_to_worker_as(
                        task.id,
                        worker.id,
                        &TaskActivityActor::worker(queen.id),
                    )
                    .unwrap();
                workers.push(worker.id);
            }
            store
                .set_manual_presence(Some(swarm_domain::PresenceMode::NightWatch), 100)
                .unwrap();
            let admitted = store.claim_coordinator_worker_wakes(100).unwrap();
            assert_eq!(admitted.len(), 1);
            assert_eq!(admitted[0].worker_id, workers[1]);
            assert!(
                store
                    .claim_coordinator_worker_wakes(101)
                    .unwrap()
                    .is_empty()
            );
            store
                .set_manual_presence(Some(swarm_domain::PresenceMode::AtHive), 102)
                .unwrap();
            let resumed = store.claim_coordinator_worker_wakes(102).unwrap();
            assert_eq!(resumed.len(), 1);
            assert_eq!(resumed[0].worker_id, workers[0]);
        }
    }

    #[test]
    fn automatic_worker_wakes_are_serialized_between_resource_checks() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let petal = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let pollen = store
            .create_worker(
                "Pollen",
                ProviderKind::ClaudeCode,
                "/workspace/pollen",
                false,
                2,
            )
            .unwrap();
        for (title, workspace, worker_id) in [
            ("Polish Petal", "/workspace/petal", petal.id),
            ("Polish Pollen", "/workspace/pollen", pollen.id),
        ] {
            let task = store.create_task(title, workspace).unwrap();
            store
                .transition_task(task.id, swarm_domain::TaskState::Ready)
                .unwrap();
            store
                .assign_task_to_worker_as(task.id, worker_id, &TaskActivityActor::worker(queen.id))
                .unwrap();
        }

        let first_pass = store.claim_coordinator_worker_wakes(100).unwrap();
        assert_eq!(first_pass.len(), usize::from(AUTOMATIC_WAKE_BATCH_LIMIT));
        assert_eq!(store.coordinator_status().unwrap().queued_actions, 2);
        assert!(
            store
                .complete_coordinator_worker_wake(&first_pass[0].action_id, 101)
                .unwrap()
        );

        let second_pass = store.claim_coordinator_worker_wakes(130).unwrap();
        assert_eq!(second_pass.len(), usize::from(AUTOMATIC_WAKE_BATCH_LIMIT));
        assert_ne!(first_pass[0].worker_id, second_pass[0].worker_id);
        assert_eq!(store.coordinator_status().unwrap().queued_actions, 1);
    }

    #[test]
    fn operator_assignment_does_not_claim_unattended_authority() {
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
        let task = store
            .create_task("Operator-directed work", "/workspace/petal")
            .unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        assert!(
            store
                .claim_coordinator_worker_wakes(100)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn interrupted_wake_becomes_uncertain_and_never_replays() {
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
        let task = store
            .create_task("Wake safely", "/workspace/petal")
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        assert_eq!(store.claim_coordinator_worker_wakes(100).unwrap().len(), 1);
        assert_eq!(store.recover_inflight_coordinator_actions().unwrap(), 1);
        assert!(
            store
                .claim_coordinator_worker_wakes(101)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.coordinator_status().unwrap().uncertain_actions, 1);
    }

    #[test]
    fn schema_v62_wake_actions_survive_all_attention_migrations() {
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
        let task = store
            .create_task("Preserve the queued wake", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute_batch(
                "DROP INDEX coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v63;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind = 'wake_assigned_worker'),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 INSERT INTO coordinator_actions (
                     id, idempotency_key, kind, worker_id, task_id, state, reason,
                     attempts, attempted_at, finished_at, created_at, updated_at
                 ) SELECT id, idempotency_key, kind, worker_id, task_id, state, reason,
                          attempts, attempted_at, finished_at, created_at, updated_at
                   FROM coordinator_actions_v63;
                 DROP TABLE coordinator_actions_v63;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF;
                 PRAGMA user_version = 62;",
            )
            .unwrap();

        migrate_coordinator_attention(&transaction).unwrap();
        assert_eq!(
            transaction
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            63
        );
        assert_eq!(
            transaction
                .query_row(
                    "SELECT kind FROM coordinator_actions WHERE task_id = ?1",
                    [task.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "wake_assigned_worker"
        );
        let new_columns = transaction
            .prepare("PRAGMA table_info(coordinator_actions)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(new_columns.contains(&"session_id".to_owned()));
        assert!(new_columns.contains(&"evidence_revision".to_owned()));
        migrate_coordinator_worker_exit_attention(&transaction).unwrap();
        assert_eq!(
            transaction
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            64
        );
        let table_sql: String = transaction
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_actions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_sql.contains("owned_work_worker_exited_attention"));
        assert_v64_to_v65_preserves_action(&transaction, task.id);
        transaction.commit().unwrap();
    }

    #[test]
    fn delivered_ready_work_surfaces_only_after_loaded_worker_stays_resting() {
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
            .create_task("Begin the delivered work", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let dispatch = store
            .claim_task_dispatches(100, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        assert!(
            store
                .complete_task_dispatch(&dispatch.assignment_id, 101)
                .unwrap()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 90 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();

        assert!(
            store
                .assigned_ready_work_not_started_candidates(400, 300)
                .unwrap()
                .is_empty()
        );
        let candidate = store
            .assigned_ready_work_not_started_candidates(401, 300)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(candidate.worker_id, worker.id);
        assert_eq!(candidate.session_id, session);
        assert_eq!(candidate.task_id, task.id);
        assert_eq!(candidate.task_revision, 90);
        assert_eq!(candidate.age_seconds, 300);
        assert!(
            store
                .record_assigned_ready_work_not_started_attention(&candidate, 401, 300)
                .unwrap()
        );
        assert!(
            !store
                .record_assigned_ready_work_not_started_attention(&candidate, 402, 300)
                .unwrap()
        );
        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(
            attention[0].kind,
            "assigned_ready_work_not_started_attention"
        );
        assert_eq!(attention[0].worker_name, "Petal");
        assert_eq!(attention[0].age_seconds, 300);
        assert_eq!(
            store
                .coordinator_status()
                .unwrap()
                .unstarted_attention_actions,
            1
        );

        store.transition_task(task.id, TaskState::Active).unwrap();
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
    }

    /// A row already on the board clears itself once the worker starts.
    ///
    /// The creation guard cannot do this: by the time the worker acts, the row
    /// exists. Queen watched a live-computed age climb past an hour on exactly
    /// this shape, and the note in `LIVE_ATTENTION_SOURCE` records the same
    /// mistake being made once already for the busy-worker case.
    #[test]
    fn delivered_ready_work_attention_clears_when_the_worker_acts_on_the_task() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Bramble",
                ProviderKind::ClaudeCode,
                "/workspace/bramble",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Raise it, then work it", "/workspace/bramble")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let dispatch = store
            .claim_task_dispatches(100, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        assert!(
            store
                .complete_task_dispatch(&dispatch.assignment_id, 101)
                .unwrap()
        );
        let candidate = store
            .assigned_ready_work_not_started_candidates(401, 300)
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            store
                .record_assigned_ready_work_not_started_attention(&candidate, 401, 300)
                .unwrap()
        );
        assert_eq!(
            store.current_coordinator_attention(402).unwrap().len(),
            1,
            "the row is on the board and was right to be raised"
        );

        // The worker starts, without ever transitioning the task.
        store
            .amend_task_facts(task.id, worker.id, "Picked this up; tracing the query.")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "UPDATE task_amendments SET created_at = 450;
                 UPDATE task_activity SET occurred_at = 450 WHERE kind = 'amended';",
            )
            .unwrap();

        assert!(
            store.current_coordinator_attention(451).unwrap().is_empty(),
            "the worker acting on the task is what clears this, with nothing \
             having to be dismissed by hand"
        );
    }

    /// A worker working a Ready task it never transitioned is not ignoring it.
    ///
    /// This is the shape behind eleven of the twelve instances Queen recorded.
    /// The flag asked "has this been moved to Active yet" and read the answer as
    /// "is anyone working it". They are the same question only for a task nobody
    /// has touched.
    ///
    /// The amendment is the load-bearing part. It writes no `task_activity` row
    /// and does not bump `tasks.updated_at`, so before this fix there was no clock
    /// in the schema that could see it -- which is exactly why the work was
    /// invisible while it was happening.
    ///
    /// Both halves matter and the second is why this is not just silencing:
    /// while the worker is working, nothing is raised; once it STOPS, the flag
    /// fires again on the time since it stopped. That is Queen's instance
    /// twelve, which was right for the wrong reason and now has a clock that
    /// means what it says.
    #[test]
    fn delivered_ready_work_is_quiet_while_worked_and_fires_once_it_stops() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Thistle",
                ProviderKind::ClaudeCode,
                "/workspace/thistle",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Work it without moving it", "/workspace/thistle")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let dispatch = store
            .claim_task_dispatches(100, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        assert!(
            store
                .complete_task_dispatch(&dispatch.assignment_id, 101)
                .unwrap()
        );

        // Delivered and untouched, the flag is still right to fire. Asserting
        // this first is what stops the fix from being "never fire".
        assert_eq!(
            store
                .assigned_ready_work_not_started_candidates(401, 300)
                .unwrap()
                .len(),
            1,
            "a briefing nobody has acted on is what this flag is for"
        );

        // The worker records progress the way the operator asked it to, and
        // never transitions the task. created_at is pinned because the column
        // defaults to the real clock while this fixture runs on a synthetic one.
        store
            .amend_task_facts(task.id, worker.id, "Reproduced it; the clock is wrong.")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "UPDATE task_amendments SET created_at = 350;
                 UPDATE task_activity SET occurred_at = 350 WHERE kind = 'amended';",
            )
            .unwrap();

        assert!(
            store
                .assigned_ready_work_not_started_candidates(401, 300)
                .unwrap()
                .is_empty(),
            "a worker that acted on the task 51 seconds ago has not ignored it"
        );

        // It stops. Now there IS something to say, and the number attached to
        // it is the time since it stopped -- not the 599 seconds since delivery,
        // which is the figure that had Queen reading transcripts.
        let candidate = store
            .assigned_ready_work_not_started_candidates(700, 300)
            .unwrap()
            .pop()
            .expect("work abandoned mid-way still has to surface");
        assert_eq!(candidate.task_id, task.id);
        assert_eq!(
            candidate.age_seconds, 350,
            "age is measured from the last thing the worker did, not from delivery"
        );
    }

    #[test]
    fn delivered_ready_work_attention_rechecks_revision_and_engagement() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/workspace/clover",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Start after briefing", "/workspace/clover")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let dispatch = store
            .claim_task_dispatches(100, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        assert!(
            store
                .complete_task_dispatch(&dispatch.assignment_id, 101)
                .unwrap()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 90 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();

        store
            .renew_worker_engagement(session, None, 401, 300)
            .unwrap();
        assert!(
            store
                .assigned_ready_work_not_started_candidates(401, 300)
                .unwrap()
                .is_empty()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "DELETE FROM worker_engagements WHERE worker_id = ?1",
                [worker.id.to_string()],
            )
            .unwrap();
        let candidate = store
            .assigned_ready_work_not_started_candidates(401, 300)
            .unwrap()
            .pop()
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 400 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();
        assert!(
            !store
                .record_assigned_ready_work_not_started_attention(&candidate, 401, 300)
                .unwrap()
        );
    }

    #[test]
    fn work_waiting_on_an_operator_decision_is_not_stale() {
        // Observed on 2026-08-18: the detector raised stale attention against
        // the BFG Operations worker while that worker was neither stuck nor
        // crashed. It had filed a decision and correctly stopped, because the
        // answer was not its to give. "Active work is unchanged while its
        // loaded worker is resting" describes a fault; waiting on the operator
        // is the system working.
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Petal", 100);

        assert_eq!(
            store.stale_owned_work_candidates(1_000, 600).unwrap().len(),
            1,
            "unchanged active work is stale while nothing is pending"
        );

        store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker,
                task_id: Some(task),
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Which reading of the number is right?",
                summary: "Whether to proceed, and what it costs if we do not.",
                reason: "The console and the wire disagree.",
                risk: "",
                evidence: "",
                suggested_action: "Treat the wire as authoritative",
                allowed_actions: &["Treat the wire as authoritative".to_owned()],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();

        assert!(
            store
                .stale_owned_work_candidates(1_000, 600)
                .unwrap()
                .is_empty(),
            "work waiting on an operator answer must not be reported as stale"
        );
    }

    #[test]
    fn stale_owned_work_requires_loaded_unengaged_active_ownership() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, session, task) = active_owned_work(&store, "Petal", 100);

        let candidate = store.stale_owned_work_candidates(1_000, 600).unwrap();
        assert_eq!(candidate.len(), 1);
        assert_eq!(candidate[0].worker_id, worker);
        assert_eq!(candidate[0].session_id, session);
        assert_eq!(candidate[0].task_id, task);
        assert_eq!(candidate[0].task_revision, 100);
        assert_eq!(candidate[0].age_seconds, 900);

        store
            .renew_worker_engagement(session, None, 1_000, 300)
            .unwrap();
        assert!(
            store
                .stale_owned_work_candidates(1_001, 600)
                .unwrap()
                .is_empty()
        );
    }

    /// The two blocks on the board today do NOT reach the operator, and one
    /// that has genuinely stalled does.
    ///
    /// Modelled on the real population rather than an invented one: both #1714
    /// tasks exceed twelve hours AND are waiting on a zero-traffic window with
    /// hours left, so both are correct and unactionable. If the first two things
    /// this channel ever says cannot be acted on, its reader learns to dismiss
    /// it -- which is the failure this Hive spent a night removing from a
    /// different flag.
    #[test]
    fn a_block_waiting_on_a_moment_that_has_not_arrived_does_not_reach_the_operator() {
        let store = TaskStore::in_memory().unwrap();
        let twelve_hours = 12 * 60 * 60;
        let now = 1_000_000;

        let blocked = |title: &str, note: &str, blocked_at: i64| {
            let worker = store
                .create_worker(title, ProviderKind::ClaudeCode, "/workspace/w", false, 1)
                .unwrap();
            let session = WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            let task = store.create_task(title, "/workspace/w").unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store
                .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
                .unwrap();
            store.transition_task(task.id, TaskState::Active).unwrap();
            store
                .transition_task_with_note(task.id, TaskState::Blocked, note)
                .unwrap();
            store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE task_activity SET occurred_at = ?2
                     WHERE task_id = ?1 AND to_state = 'blocked'",
                    params![task.id.to_string(), blocked_at],
                )
                .unwrap();
            task.id
        };

        // 72 hours blocked, waiting on a window that opens in the future.
        let waiting = blocked(
            "Platform 1714 steps 4-5",
            "Zero-traffic window.\nBlocked until: 2026-08-27T17:35:33Z",
            now - 72 * 60 * 60,
        );
        // 18 hours blocked, nothing checkable named. This is the one that has
        // actually stalled.
        let stalled = blocked(
            "Stalled on nobody",
            "Blocked on Queen deciding",
            now - 18 * 60 * 60,
        );

        let reaching: Vec<_> = store
            .operator_block_escalation_candidates(now, twelve_hours)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.task_id)
            .collect();
        assert!(
            reaching.contains(&stalled),
            "a block that named nothing and has waited 18 hours must reach the operator"
        );
        assert!(
            !reaching.contains(&waiting),
            "a block waiting on a moment that has not arrived is correct and \
             unactionable, and must not be the first thing this channel says"
        );

        // The moment the named condition passes, it is no longer waiting: it is
        // a block whose reason expired and nobody came back.
        let after = 1_787_852_133 + 1;
        let reaching_later: Vec<_> = store
            .operator_block_escalation_candidates(after, twelve_hours)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.task_id)
            .collect();
        assert!(
            reaching_later.contains(&waiting),
            "an elapsed deadline escalates -- the reason expired and nobody came back"
        );
    }

    /// A NOTE IS NOT A MOVE. Annotating a blocked task must not reset its clock.
    ///
    /// The live case: 01a040e4 had been blocked for 10.8 hours and read 0.4
    /// after one correction, an hour short of the operator's threshold. The
    /// escalation exists to reach them when Queen is the bottleneck, and a Queen
    /// who annotates blocked work -- which is what a conscientious Queen does --
    /// was silencing the alarm built to catch exactly that.
    ///
    /// The cause is that `swarm_correct_task_record` writes `kind='corrected'`
    /// with `to_state` set to the task's CURRENT state, so on a blocked task the
    /// note is indistinguishable from a re-block and `MAX()` prefers it.
    #[test]
    fn annotating_a_blocked_task_does_not_restart_its_clock() {
        let store = TaskStore::in_memory().unwrap();
        let twelve_hours = 12 * 60 * 60;
        let now = 1_000_000;
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
            .create_task("Waiting on a release", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .transition_task_with_note(task.id, TaskState::Blocked, "Blocked on the operator")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_activity SET occurred_at = ?2
                 WHERE task_id = ?1 AND to_state = 'blocked' AND kind = 'state_changed'",
                params![task.id.to_string(), now - 13 * 60 * 60],
            )
            .unwrap();

        assert_eq!(
            store
                .operator_block_escalation_candidates(now, twelve_hours)
                .unwrap()
                .len(),
            1,
            "thirteen hours blocked is past the threshold"
        );

        // Queen writes about it, conscientiously, and moves nothing. This is the
        // exact call that suppressed it on the live board.
        store
            .append_task_correction(
                task.id,
                "Still waiting on the operator.",
                &TaskActivityActor::operator(),
            )
            .unwrap();

        let candidate = store
            .operator_block_escalation_candidates(now, twelve_hours)
            .unwrap()
            .pop()
            .expect("a note is attention, not action -- the block is still thirteen hours old");
        assert_eq!(candidate.task_id, task.id);
        assert_eq!(
            candidate.age_seconds,
            13 * 60 * 60,
            "the age is measured from the block, not from the last thing written about it"
        );
    }

    /// Escalating grants NO authority. The listing moves nothing.
    ///
    /// The operator asked not to lose "the design of the queen being an
    /// arbitrator and keeping workers going", so the escalation is a read and
    /// stays a read. Asserted rather than assumed, because the cheap way to
    /// make an escalation useful is to let it act, and that is the thing that
    /// was explicitly ruled out.
    #[test]
    fn reaching_the_operator_does_not_move_the_task() {
        let store = TaskStore::in_memory().unwrap();
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
        let task = store.create_task("Waiting", "/workspace/petal").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .transition_task_with_note(task.id, TaskState::Blocked, "Blocked on Queen deciding")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_activity SET occurred_at = ?2
                 WHERE task_id = ?1 AND to_state = 'blocked'",
                params![task.id.to_string(), 0],
            )
            .unwrap();

        assert_eq!(
            store
                .operator_block_escalation_candidates(100_000, 12 * 60 * 60)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.get_task(task.id).unwrap().state,
            TaskState::Blocked,
            "the escalation reports; it does not unblock"
        );
    }

    /// Twelve hours, and the age comes from the transition rather than the row.
    #[test]
    fn the_operator_hears_at_twelve_hours_measured_from_the_transition() {
        let store = TaskStore::in_memory().unwrap();
        let twelve_hours = 12 * 60 * 60;
        let now = 1_000_000;
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
        let task = store.create_task("Waiting", "/workspace/petal").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .transition_task_with_note(task.id, TaskState::Blocked, "Blocked on Queen deciding")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_activity SET occurred_at = ?2
                 WHERE task_id = ?1 AND to_state = 'blocked'",
                params![task.id.to_string(), now - twelve_hours + 1],
            )
            .unwrap();

        assert!(
            store
                .operator_block_escalation_candidates(now, twelve_hours)
                .unwrap()
                .is_empty(),
            "one second short of twelve hours is not twelve hours"
        );

        // A SWEEP TOUCHES THE ROW. Measuring from updated_at would restart the
        // clock here and the block would never age past the threshold.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
                params![task.id.to_string(), now + 5],
            )
            .unwrap();
        let candidate = store
            .operator_block_escalation_candidates(now + 1, twelve_hours)
            .unwrap()
            .pop()
            .expect("twelve hours from the TRANSITION, whatever touched the row since");
        assert_eq!(candidate.task_id, task.id);
        assert_eq!(candidate.age_seconds, twelve_hours);
    }

    /// Recording work unverifiable is not recording it verified.
    ///
    /// The operator ruled for this control after nineteen finished tasks sat in
    /// a panel that asked for evidence and offered no way to give any. The
    /// danger in building it is obvious and is the whole reason the assertions
    /// below exist: a control that quietly counted as evidence would empty the
    /// panel and destroy the difference between "finished" and "shown to be
    /// live" that the panel exists to draw.
    #[test]
    fn recording_work_unverifiable_never_counts_as_evidence_and_never_moves_it() {
        let store = TaskStore::in_memory().unwrap();
        let (_worker, _session, task) = active_owned_work(&store, "Marigold", 100);
        let before = store.get_task(task).unwrap();

        assert!(
            store
                .record_task_unverifiable(
                    task,
                    "Ten days old, in another repo, no run to point at.",
                    2_000
                )
                .unwrap()
        );
        let after = store.get_task(task).unwrap();

        assert!(
            after.closed_unverifiable,
            "the record exists and the board can see it"
        );
        assert!(
            !after.closed_on_evidence,
            "and it is NOT evidence -- nobody checked, so nothing may render this as verified"
        );
        assert!(
            !after.deployment_recorded,
            "and it certainly is not a deployment"
        );
        assert_eq!(
            (before.state, before.updated_at),
            (after.state, after.updated_at),
            "it says what is knowable about the work, so it must not rewrite what happened to it"
        );

        assert!(
            store
                .list_task_activity(task, 50)
                .unwrap()
                .events
                .iter()
                .any(|event| event.note.contains("Recorded as unverifiable")
                    && event.actor_kind == swarm_domain::TaskActivityActorKind::Operator),
            "and it is in the trail, attributed to the operator who asserted it"
        );

        assert!(
            !store
                .record_task_unverifiable(task, "again", 2_100)
                .unwrap(),
            "recording it twice changes nothing rather than stacking records"
        );
    }

    /// Work that HAS evidence is not unverifiable, and saying so would be false.
    #[test]
    fn work_with_a_recorded_deployment_cannot_be_called_unverifiable() {
        let store = TaskStore::in_memory().unwrap();
        let (_worker, _session, task) = active_owned_work(&store, "Marigold", 100);
        // A deployment is only accepted once the work is finished or handed
        // off, so the fixture's Active task has to get there first.
        store.transition_task(task, TaskState::Review).unwrap();
        store
            .record_task_deployment(task, "production", "sha abc123", 2_000_000_000)
            .unwrap();

        assert!(
            matches!(
                store.record_task_unverifiable(task, "cannot check", 2_000_000_001),
                Err(TaskStoreError::CompletionEvidenceRequired)
            ),
            "refused rather than silently overwritten, so the two records can never disagree"
        );
    }

    /// THE EVIDENCE IS BESIDE THE NUMBER, and the number is unchanged.
    ///
    /// Queen read 9002 seconds on a stale row and asked a worker whether it had
    /// stalled. It had not: two hours into shipping seven of nine subsystems,
    /// with 32 notes recorded, the newest a minute old. Her own diagnosis is
    /// the design — "the row's prose is right while its number is wrong, and
    /// the number is what makes a coordinator ask."
    ///
    /// So notes still buy no quiet, which the test below defends, and the row
    /// now carries what a coordinator would otherwise have to open the task to
    /// find. Both halves are asserted here because fixing one by breaking the
    /// other is the obvious wrong move: counting notes would have suppressed
    /// the flag entirely.
    #[test]
    fn a_stale_row_carries_the_notes_that_do_not_clear_it() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Sorrel", 100);

        for note in ["Seven of nine subsystems are live.", "Eighth deployed."] {
            store.record_task_note(task, worker, note).unwrap();
        }
        store
            .connection()
            .unwrap()
            .execute_batch("UPDATE task_activity SET occurred_at = 1_540 WHERE kind = 'noted';")
            .unwrap();

        let candidate = store
            .stale_owned_work_candidates(1_600, 600)
            .unwrap()
            .pop()
            .expect("notes buy no quiet, so the row still fires");
        assert_eq!(
            candidate.age_seconds, 1_500,
            "and the age is untouched — measured from the last ACTION, not the last note"
        );

        store
            .record_stale_owned_work_attention(
                &candidate,
                1_600,
                600,
                BackgroundWorkReading::NoneVisible,
            )
            .unwrap();

        let reason: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT reason FROM coordinator_actions
                 WHERE kind = 'stale_owned_work_attention' AND task_id = ?1",
                [task.to_string()],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            reason.contains("2 notes"),
            "the coordinator sees the evidence without opening the task: {reason}"
        );
        assert!(
            reason.contains("moments ago"),
            "and how recent it is, which is the part that says whether to interrupt: {reason}"
        );
        assert!(
            reason.contains("do not clear this flag on purpose"),
            "and why the age is high anyway, so the row is not read as a contradiction: {reason}"
        );
    }

    /// A note is a record, not an alibi.
    ///
    /// The worry when this was filed was that a progress note becomes a way to
    /// look busy. It cannot: `last_task_action_source!` counts `corrected`,
    /// `details_updated` and `amended`, and `noted` is deliberately absent, so
    /// work that stops changing is still reported as unchanged however much
    /// its worker writes. The answer is structural rather than a rule someone
    /// has to remember.
    ///
    /// The second half is the control. Without it this test would pass just as
    /// happily if the stale query had stopped working altogether, which is the
    /// failure shape this repository keeps producing.
    #[test]
    fn a_note_does_not_hold_off_the_stale_flag_although_an_amendment_does() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Sorrel", 100);
        assert_eq!(
            store.stale_owned_work_candidates(1_000, 600).unwrap().len(),
            1,
            "work nobody has touched in 900 seconds is what this flag is for"
        );

        store
            .record_task_note(
                task,
                worker,
                "Prediction before the code exists: p50 falls below 400ms, and if it does not \
                 the cache is not the bottleneck and this approach is wrong.",
            )
            .unwrap();
        // occurred_at defaults to the real clock while this fixture runs on a
        // synthetic one.
        store
            .connection()
            .unwrap()
            .execute_batch("UPDATE task_activity SET occurred_at = 900 WHERE kind = 'noted';")
            .unwrap();

        assert_eq!(
            store.stale_owned_work_candidates(1_000, 600).unwrap().len(),
            1,
            "a note must buy no quiet at all, or writing one becomes a way to look busy"
        );

        // THE CONTROL: the same shape of write, in a kind that IS an action,
        // does stop it. Without this the assertion above could be measuring a
        // stale query that no longer works rather than the kind.
        store
            .amend_task_facts(
                task,
                worker,
                "The 400ms figure came from staging, not production.",
            )
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "UPDATE task_amendments SET created_at = 900;
                 UPDATE task_activity SET occurred_at = 900 WHERE kind = 'amended';",
            )
            .unwrap();
        assert!(
            store
                .stale_owned_work_candidates(1_000, 600)
                .unwrap()
                .is_empty(),
            "an amendment does hold it off, so the assertion above measures the KIND rather than \
             a query that has stopped selecting anything"
        );
    }

    /// The workaround this replaced told the board something false.
    ///
    /// A worker asked to state a prediction before writing code had two ways
    /// to say anything -- finish, or change state -- so it moved its own task
    /// to Blocked, wrote the note, and moved it back. For that interval the
    /// board said BLOCKED about work that was not blocked, which
    /// `blocked_work_unattended_attention` and Queen's triage both read.
    #[test]
    fn a_note_leaves_the_board_saying_exactly_what_it_said_before() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Sorrel", 100);
        let before = store.get_task(task).unwrap();

        store
            .record_task_note(
                task,
                worker,
                "Predicting the redirect loop is in the guard.",
            )
            .unwrap();

        let after = store.get_task(task).unwrap();
        assert_eq!(
            before.state, after.state,
            "a note must not move the task; the false Blocked row is the whole reason this exists"
        );
        assert_eq!(
            before.updated_at, after.updated_at,
            "and it must not touch the stale-work clock either"
        );

        let history = store.list_task_activity(task, 50).unwrap();
        let note = history
            .events
            .iter()
            .find(|event| event.kind == swarm_domain::TaskActivityKind::Noted)
            .expect("the note is in the trail a reader actually reads");
        assert_eq!(note.actor_kind, swarm_domain::TaskActivityActorKind::Worker);
        assert!(note.note.contains("Predicting"));
        assert!(
            note.from_state.is_none() && note.to_state.is_none(),
            "it carries no state change, because it is not one"
        );

        assert!(
            store
                .amendments_for_tasks(&[task])
                .unwrap()
                .get(&task)
                .is_none_or(std::vec::Vec::is_empty),
            "and it is NOT an amendment: every listing carries those beside the description under \
             \"believe this over it\", which is wrong for a claim the outcome may falsify"
        );

        assert!(
            store
                .current_coordinator_attention(1_000)
                .unwrap()
                .is_empty(),
            "nothing about a worker writing a sentence should wake a coordinator"
        );
    }

    /// A worker amending its Active task is working it, not sitting on it.
    ///
    /// `updated_at` moves on a transition and not on an amendment, so a worker
    /// recording progress the way the board asks -- without changing state,
    /// because the state is already right -- looked untouched for as long as it
    /// kept doing it. Same blind spot the unstarted-work flag had.
    ///
    /// The second half is why this is not just silencing: once the worker
    /// STOPS, the flag fires on the time since it stopped, which is the number
    /// worth acting on rather than the time since the last state change.
    #[test]
    fn active_work_being_amended_is_not_stale_until_the_amending_stops() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Sorrel", 100);

        // Untouched since 100, read at 1000 with a 600s grace: still correct.
        assert_eq!(
            store.stale_owned_work_candidates(1_000, 600).unwrap().len(),
            1,
            "work nobody has touched in 900 seconds is what this flag is for"
        );

        // The worker records progress without transitioning. created_at is
        // pinned because the column defaults to the real clock while this
        // fixture runs on a synthetic one.
        store
            .amend_task_facts(task, worker, "Still on it; the migration is rebuilding.")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "UPDATE task_amendments SET created_at = 900;
                 UPDATE task_activity SET occurred_at = 900 WHERE kind = 'amended';",
            )
            .unwrap();

        assert!(
            store
                .stale_owned_work_candidates(1_000, 600)
                .unwrap()
                .is_empty(),
            "a worker that acted on the task 100 seconds ago has not gone quiet"
        );

        // It stops. The age is measured from the amendment, not from the
        // transition that last moved updated_at.
        let candidate = store
            .stale_owned_work_candidates(1_600, 600)
            .unwrap()
            .pop()
            .expect("work that genuinely went quiet still has to surface");
        assert_eq!(candidate.task_id, task);
        assert_eq!(
            candidate.age_seconds, 700,
            "age runs from the last thing the worker did, not the last state change"
        );
    }

    /// A worker was blamed for 27 hours for being idle on work it had never
    /// been given.
    ///
    /// The brief was claimed, the API died before it landed, and the row sat
    /// `uncertain` with no delivery. From outside that is indistinguishable
    /// from a resting worker — no commits, no files touched — so the detector
    /// reported "unchanged while its loaded worker is resting" and pointed at
    /// the wrong thing. The dispatch row held the answer all along.
    #[test]
    fn work_whose_brief_was_never_delivered_names_the_brief_not_the_worker() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Clover", 100);

        // The API dies mid-delivery: claimed, attempted, never delivered.
        let claimed = store
            .claim_task_dispatches(200, &std::collections::HashSet::new())
            .unwrap();
        assert_eq!(claimed.len(), 1, "the brief is in flight");
        assert_eq!(store.recover_inflight_task_dispatches().unwrap(), 1);

        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            store
                .record_stale_owned_work_attention(
                    &candidate,
                    1_000,
                    600,
                    BackgroundWorkReading::NoneVisible
                )
                .unwrap()
        );
        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(
            attention[0].kind, "owned_work_never_briefed_attention",
            "a brief that never landed must not be reported as an idle worker: {:?}",
            attention[0]
        );
        assert!(
            attention[0].reason.contains("never given this brief"),
            "and it must say what to do about it: {}",
            attention[0].reason
        );

        // Re-assigning is Queen's lever for stranded work, and for Active work
        // it used to bind a session and deliver nothing. It now redelivers,
        // which is the whole repair — no walking the task through `blocked` to
        // reach `ready`, which lies on the board while it happens.
        store
            .assign_task_to_worker_as(task, worker, &TaskActivityActor::operator())
            .unwrap();
        let redelivered = store
            .claim_task_dispatches(2_000, &std::collections::HashSet::new())
            .unwrap();
        assert!(
            redelivered.iter().any(|dispatch| dispatch.task_id == task),
            "re-assigning Active work must send the brief again"
        );
        store
            .complete_task_dispatch(&redelivered[0].assignment_id, 2_001)
            .unwrap();

        // And the attention clears itself once the brief lands, rather than
        // needing to be dismissed by hand.
        assert!(
            store.current_coordinator_attention(0).unwrap().is_empty(),
            "a delivered brief answers the question the attention was asking"
        );
    }

    /// Two tasks sat assigned and unreachable for twenty-one minutes, looking
    /// routed on the board, until the operator asked from the other side why he
    /// could not see those workers in the list. They were not running, and
    /// assignment to a sleeping worker creates no session binding and no
    /// briefing, so nothing anywhere said the work could not be delivered.
    ///
    /// The control matters as much as the case. The original report was made
    /// twice with `pgrep -af <worker id>`, which matched the reporting shell's
    /// own command line and so could never answer NO. This asserts both
    /// directions against `worker_sessions`.
    #[test]
    fn work_assigned_to_a_worker_that_is_not_running_is_visible_as_such() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let sleeping = store
            .create_worker(
                "RCG Hub",
                ProviderKind::ClaudeCode,
                "/workspace/hub",
                false,
                1,
            )
            .unwrap();
        let running = store
            .create_worker(
                "Scout",
                ProviderKind::ClaudeCode,
                "/workspace/scout",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(running.id, WorkerSessionId::new())
            .unwrap();

        let parked = store
            .create_task_with_details(
                "Latent hub work",
                "",
                TaskPriority::Normal,
                "/workspace/hub",
            )
            .unwrap();
        store.transition_task(parked.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(parked.id, sleeping.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        // The control: same shape, on a worker that is actually running.
        let reachable = store
            .create_task_with_details("Scout work", "", TaskPriority::Normal, "/workspace/scout")
            .unwrap();
        store
            .transition_task(reachable.id, TaskState::Ready)
            .unwrap();
        store
            .assign_task_to_worker_as(
                reachable.id,
                running.id,
                &TaskActivityActor::worker(queen.id),
            )
            .unwrap();

        // The wake itself IS armed: assignment queues one for the sleeping
        // worker. So the promise in the tool description is kept up to this
        // point, and what failed was further along — the coordinator declining
        // to start anything and saying nothing about it.
        let wakes = store.claim_coordinator_worker_wakes(1_000).unwrap();
        assert!(
            wakes.iter().any(|wake| wake.worker_id == sleeping.id),
            "assignment must queue a wake for a sleeping worker: {wakes:?}"
        );

        let unreachable = store
            .work_assigned_to_a_worker_that_is_not_running()
            .unwrap();
        assert_eq!(
            unreachable.len(),
            1,
            "exactly the work that cannot be delivered: {unreachable:?}"
        );
        assert_eq!(unreachable[0].task_id, parked.id.to_string());
        assert_eq!(unreachable[0].worker_name, "RCG Hub");

        // And it stops being reported the moment the worker is running, so
        // starting it is the whole fix.
        store
            .bind_worker_session(sleeping.id, WorkerSessionId::new())
            .unwrap();
        assert!(
            store
                .work_assigned_to_a_worker_that_is_not_running()
                .unwrap()
                .is_empty(),
            "a worker that is up must not still be reported as unreachable"
        );
    }

    /// The age has to advance with the clock, and a single call cannot tell.
    ///
    /// It was written once at observation and returned verbatim ever after, so
    /// it read like a live age and was not one. Measured 2026-08-23: 303s
    /// reported for a condition 176 minutes old, and one value moved DOWNWARD
    /// between two calls. Queen read it, concluded the briefs were five minutes
    /// old and so not yet evidence of a stall, and wrote that into a live
    /// operator decision record.
    ///
    /// Two calls with a real gap between them, which is the only shape that
    /// catches it — that is why it survived.
    #[test]
    fn attention_age_advances_with_the_clock_and_never_goes_backwards() {
        let store = TaskStore::in_memory().unwrap();
        let (_, _, _task) = active_owned_work(&store, "Clover", 100);
        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            store
                .record_stale_owned_work_attention(
                    &candidate,
                    1_000,
                    600,
                    BackgroundWorkReading::NoneVisible
                )
                .unwrap()
        );

        let first = store.current_coordinator_attention(1_000).unwrap();
        assert_eq!(first.len(), 1);
        let action_id = first[0].action_id.clone();
        // Not zero at the moment of observation: the work had already been
        // unchanged for fifteen minutes when the detector fired, and reporting
        // only the time SINCE would throw that away on every read.
        assert_eq!(
            first[0].age_seconds, 900,
            "the first read must carry the age the condition already had"
        );

        // Half an hour later, on the same unchanged record.
        let elapsed = 1_800;
        let second = store
            .current_coordinator_attention(1_000 + elapsed)
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].action_id, action_id, "the same record");
        assert_eq!(
            second[0].age_seconds - first[0].age_seconds,
            elapsed,
            "age must advance by the wall clock, not sit at its observation value"
        );
        assert_eq!(
            second[0].observed_at, first[0].observed_at,
            "and observed_at must not move, so the two can be reconciled"
        );

        // Never backwards, which is the anomaly actually observed.
        let mut previous = 0;
        for step in [0, 60, 900, 5_000, 10_579] {
            let age = store.current_coordinator_attention(1_000 + step).unwrap()[0].age_seconds;
            assert!(
                age >= previous,
                "age went backwards: {previous} then {age} at +{step}"
            );
            previous = age;
        }
        // The reported case: 176 minutes elapsed since the observation. It must
        // read in hours, not the 303 seconds Queen was given.
        assert_eq!(previous, 900 + 10_579);
        assert!(
            previous > 60 * 60 * 2,
            "at over two hours old it must not read as minutes: {previous}"
        );
    }

    #[test]
    fn stale_attention_is_revision_bound_visible_and_idempotent() {
        let store = TaskStore::in_memory().unwrap();
        let (_, _, task) = active_owned_work(&store, "Clover", 100);
        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();

        assert!(
            store
                .record_stale_owned_work_attention(
                    &candidate,
                    1_000,
                    600,
                    BackgroundWorkReading::NoneVisible
                )
                .unwrap()
        );
        assert!(
            !store
                .record_stale_owned_work_attention(
                    &candidate,
                    1_001,
                    600,
                    BackgroundWorkReading::NoneVisible
                )
                .unwrap()
        );
        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].worker_name, "Clover");
        assert_eq!(attention[0].task_title, "Keep the release moving");
        assert_eq!(attention[0].age_seconds, 900);
        let status = store.coordinator_status().unwrap();
        assert_eq!(status.completed_actions, 1);
        assert_eq!(status.stale_attention_actions, 1);
        assert_eq!(status.queen_calls_avoided, 0);

        store
            .transition_task_with_note(task, TaskState::Review, "Ready for review")
            .unwrap();
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
    }

    #[test]
    fn stale_attention_rechecks_revision_before_recording() {
        let store = TaskStore::in_memory().unwrap();
        let (_, _, task) = active_owned_work(&store, "Daisy", 100);
        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 900 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();

        assert!(
            !store
                .record_stale_owned_work_attention(
                    &candidate,
                    1_000,
                    600,
                    BackgroundWorkReading::NoneVisible
                )
                .unwrap()
        );
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
    }

    #[test]
    fn exited_worker_attention_is_grace_perioded_revision_bound_and_clears_on_recovery() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, session, task) = active_owned_work(&store, "Poppy", 100);
        assert!(store.release_worker_session(session).unwrap());
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_sessions SET ended_at = 400 WHERE session_id = ?1",
                [session.to_string()],
            )
            .unwrap();

        assert!(
            store
                .exited_worker_owned_work_candidates(699, 300)
                .unwrap()
                .is_empty()
        );
        let candidate = store
            .exited_worker_owned_work_candidates(700, 300)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(candidate.worker_id, worker);
        assert_eq!(candidate.session_id, session);
        assert_eq!(candidate.task_id, task);
        assert_eq!(candidate.task_revision, 100);
        assert_eq!(candidate.age_seconds, 300);
        assert!(
            store
                .record_exited_worker_owned_work_attention(&candidate, 700, 300)
                .unwrap()
        );
        assert!(
            !store
                .record_exited_worker_owned_work_attention(&candidate, 701, 300)
                .unwrap()
        );

        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].kind, "owned_work_worker_exited_attention");
        assert_eq!(attention[0].worker_name, "Poppy");
        assert_eq!(attention[0].age_seconds, 300);
        let first_action_id = attention[0].action_id.clone();
        let status = store.coordinator_status().unwrap();
        assert_eq!(status.worker_exit_attention_actions, 1);
        assert_eq!(status.stale_attention_actions, 0);

        let replacement = WorkerSessionId::new();
        store.bind_worker_session(worker, replacement).unwrap();
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());

        assert!(store.release_worker_session(replacement).unwrap());
        let replacement_candidate = store
            .exited_worker_owned_work_candidates(i64::MAX / 2, 0)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(replacement_candidate.session_id, replacement);
        assert!(
            store
                .record_exited_worker_owned_work_attention(&replacement_candidate, i64::MAX / 2, 0,)
                .unwrap()
        );
        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_ne!(attention[0].action_id, first_action_id);
    }

    #[test]
    fn exited_worker_attention_rechecks_task_revision_before_recording() {
        let store = TaskStore::in_memory().unwrap();
        let (_, session, task) = active_owned_work(&store, "Aster", 100);
        assert!(store.release_worker_session(session).unwrap());
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_sessions SET ended_at = 400 WHERE session_id = ?1",
                [session.to_string()],
            )
            .unwrap();
        let candidate = store
            .exited_worker_owned_work_candidates(700, 300)
            .unwrap()
            .pop()
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 650 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();

        assert!(
            !store
                .record_exited_worker_owned_work_attention(&candidate, 700, 300)
                .unwrap()
        );
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
    }
}

/// A decision nobody answered by the time its asker said it needed one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverdueDecisionCandidate {
    pub decision_id: String,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub title: String,
    pub overdue_seconds: i64,
}

impl TaskStore {
    /// Decisions whose deadline has passed while they are still pending.
    ///
    /// The operator's ruling: "queen should make this a needs you item." Until
    /// now a deadline was recorded, shown on the roster as "overdue", and acted
    /// on by nobody — the worker held indefinitely and Queen was never told.
    ///
    /// Two limits, both deliberate rather than overlooked. A decision with no
    /// task cannot be recorded, because a coordinator action must name one; one
    /// of sixteen on this Hive is like that, and inventing a task to hang it
    /// off would be worse than not raising it. A worker that has never held a
    /// session is skipped for the same reason — the attention inbox joins one,
    /// and a worker that has never run has not asked anything either.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn overdue_decision_candidates(
        &self,
        now: i64,
    ) -> Result<Vec<OverdueDecisionCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT request.id, request.requesting_worker_id, request.task_id, request.title,
                    task.updated_at, ?1 - request.deadline,
                    (SELECT session.session_id FROM worker_sessions session
                     WHERE session.worker_id = request.requesting_worker_id
                     ORDER BY session.started_at DESC, session.session_id DESC LIMIT 1)
             FROM decision_requests request
             JOIN tasks task ON task.id = request.task_id
             WHERE request.state = 'pending'
               AND request.deadline IS NOT NULL
               AND request.deadline <= ?1
               AND task.removed_at IS NULL
             ORDER BY request.deadline",
        )?;
        let rows = statement.query_map([now], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut candidates = Vec::new();
        for row in rows {
            let (decision_id, worker_id, task_id, title, task_revision, overdue_seconds, session) =
                row?;
            let (Ok(worker_id), Ok(task_id)) =
                (WorkerId::from_str(&worker_id), TaskId::from_str(&task_id))
            else {
                continue;
            };
            let Some(session_id) = session.and_then(|id| WorkerSessionId::from_str(&id).ok())
            else {
                continue;
            };
            candidates.push(OverdueDecisionCandidate {
                decision_id,
                worker_id,
                session_id,
                task_id,
                task_revision,
                title,
                overdue_seconds,
            });
        }
        Ok(candidates)
    }

    /// Tells Queen that a decision has gone unanswered past its deadline.
    ///
    /// Re-stamps rather than duplicating. The inbox only counts an observation
    /// whose evidence still matches the task it was made against, so a task
    /// that moved on would otherwise silence an attention whose decision is
    /// still waiting — and the idempotency key would stop a second one being
    /// raised. Refreshing the revision keeps one row alive for one decision.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_overdue_decision_attention(
        &self,
        candidate: &OverdueDecisionCandidate,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // Not INSERT OR IGNORE: that suppresses a CHECK violation exactly as it
        // suppresses a duplicate, which is how an unadmitted kind once did
        // nothing and said nothing.
        let changed = transaction.execute(
            "INSERT INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'decision_deadline_passed_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'A decision passed the deadline its asker set',
                     unixepoch(), unixepoch())
             ON CONFLICT(idempotency_key) DO UPDATE
                 SET evidence_revision = excluded.evidence_revision,
                     observed_age_seconds = excluded.observed_age_seconds,
                     updated_at = unixepoch()
                 WHERE coordinator_actions.evidence_revision <> excluded.evidence_revision",
            params![
                Uuid::now_v7().to_string(),
                format!("decision-deadline:{}", candidate.decision_id),
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.overdue_seconds,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}

/// Something the coordinator wanted to do and could not, with how long it has
/// been true.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorRefusal {
    pub kind: String,
    pub subject: String,
    pub worker_id: Option<WorkerId>,
    pub worker_name: Option<String>,
    pub reason: String,
    pub first_observed_at: i64,
    pub last_observed_at: i64,
    pub observations: i64,
}

/// A delivery that cannot be written because the session has an unanswered
/// provider question.
pub const REFUSAL_DELIVERY_HELD: &str = "delivery_held_open_prompt";

/// A delivery that cannot be written because the prompt already holds text
/// that was typed and never sent.
///
/// Separate from [`REFUSAL_DELIVERY_HELD`] because the remedy is the opposite
/// one. There is no question to answer — the operator has to clear a line they
/// left behind. Told to answer a prompt that is not there, they open the
/// terminal, see nothing to do, and close it again, which is what happened for
/// three hours on 2026-08-23 while the board sat at zero active tasks.
pub const REFUSAL_DELIVERY_HELD_UNSENT_TEXT: &str = "delivery_held_unsent_text";

/// A wake whose outcome could not be confirmed, and which will not replay.
///
/// The work is assigned and was never started, and nothing about the task says
/// so — it reads as routed. Deliberately not retried: a worker woken twice gets
/// its briefing twice.
/// Assigned work whose worker is not running, so no briefing can reach it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct UnreachableAssignment {
    pub task_id: String,
    pub title: String,
    pub worker_id: String,
    pub worker_name: String,
    pub assigned_at: i64,
}

pub const REFUSAL_WAKE_UNCERTAIN: &str = "wake_uncertain";
/// The coordinator was not admitted to start anything, so queued wakes were
/// not even attempted.
pub const REFUSAL_WAKE_NOT_ADMITTED: &str = "wake_not_admitted";

impl TaskStore {
    /// Work that is assigned and cannot be reached, because the worker holding
    /// it is not running.
    ///
    /// Assignment to a sleeping worker creates no session binding and no
    /// briefing — there is nowhere to put one — so the task carries an owner
    /// and nothing else. On the board that is indistinguishable from work whose
    /// brief is simply queued behind something, which is how two tasks sat
    /// assigned and unreachable for twenty-one minutes while the operator asked
    /// why he could not see those workers in the list.
    ///
    /// Answers from `worker_sessions`, so it can say NO. The report that led
    /// here was made twice with `pgrep -af <worker id>`, which matched the
    /// reporting shell's own command line and could never return a negative.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn work_assigned_to_a_worker_that_is_not_running(
        &self,
    ) -> Result<Vec<UnreachableAssignment>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.id, task.title, worker.id, worker.name, task.updated_at
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             WHERE task.state = 'ready' AND task.removed_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions session
                   WHERE session.worker_id = worker.id AND session.ended_at IS NULL
               )
             ORDER BY task.updated_at, task.id LIMIT 32",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(UnreachableAssignment {
                task_id: row.get::<_, String>(0)?,
                title: row.get(1)?,
                worker_id: row.get::<_, String>(2)?,
                worker_name: row.get(3)?,
                assigned_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Records that the coordinator declined to act, or that it is still
    /// declining.
    ///
    /// One row per subject rather than one per attempt. A stranded prompt is
    /// retried every thirty seconds, and the useful statement is "held since
    /// 01:49, 1503 checks" rather than 1503 rows nobody will read.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_coordinator_refusal(
        &self,
        kind: &str,
        subject: &str,
        worker_id: Option<WorkerId>,
        session_id: Option<WorkerSessionId>,
        reason: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if matches!(
            kind,
            REFUSAL_DELIVERY_HELD | REFUSAL_DELIVERY_HELD_UNSENT_TEXT
        ) {
            if let Some(assignment_id) = subject.strip_prefix("task-dispatch:") {
                // A scoped observation replaces the legacy task-wide hold only
                // while that exact assignment still owns this delivery.
                transaction.execute(
                    "UPDATE coordinator_refusals SET cleared_at = ?4
                     WHERE kind IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                       AND cleared_at IS NULL AND subject = (
                         SELECT 'task-brief:' || dispatch.task_id FROM task_dispatches dispatch
                         JOIN task_assignments assignment ON assignment.id = dispatch.assignment_id
                         WHERE dispatch.assignment_id = ?1 AND dispatch.worker_id = ?2
                           AND assignment.worker_session_id = ?3 AND assignment.released_at IS NULL
                           AND dispatch.state IN ('queued','dispatching'))",
                    params![
                        assignment_id,
                        worker_id.map(|id| id.to_string()),
                        session_id.map(|id| id.to_string()),
                        now
                    ],
                )?;
            }
            // A newer observation replaces the other prompt condition for the
            // same delivery. Never clear unrelated subjects or wake recovery.
            transaction.execute(
                "UPDATE coordinator_refusals SET cleared_at = ?3
                 WHERE subject = ?1 AND kind != ?2
                   AND kind IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                   AND cleared_at IS NULL",
                params![subject, kind, now],
            )?;
        }
        transaction.execute(
            "INSERT INTO coordinator_refusals
                 (kind, subject, worker_id, session_id, reason,
                  first_observed_at, last_observed_at, observations, cleared_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1, NULL)
             ON CONFLICT(kind, subject) DO UPDATE SET
                 last_observed_at = excluded.last_observed_at,
                 observations = CASE
                     WHEN coordinator_refusals.cleared_at IS NULL
                       AND coordinator_refusals.worker_id IS excluded.worker_id
                       AND coordinator_refusals.session_id IS excluded.session_id
                     THEN coordinator_refusals.observations + 1
                     ELSE 1
                 END,
                 -- A refusal that had cleared and is happening again is a new
                 -- occurrence, not a continuation of the old one.
                 first_observed_at = CASE
                     WHEN coordinator_refusals.cleared_at IS NULL
                       AND coordinator_refusals.worker_id IS excluded.worker_id
                       AND coordinator_refusals.session_id IS excluded.session_id
                     THEN coordinator_refusals.first_observed_at
                     ELSE excluded.first_observed_at
                 END,
                 reason = excluded.reason,
                 worker_id = excluded.worker_id,
                 session_id = excluded.session_id,
                 cleared_at = NULL",
            params![
                kind,
                subject,
                worker_id.map(|id| id.to_string()),
                session_id.map(|id| id.to_string()),
                reason,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Records that whatever was blocking has stopped blocking.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn clear_coordinator_refusal(
        &self,
        kind: &str,
        subject: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE coordinator_refusals SET cleared_at = ?3
             WHERE kind = ?1 AND subject = ?2 AND cleared_at IS NULL",
            params![kind, subject, now],
        )? > 0)
    }

    /// Legacy freshness window for non-prompt refusal kinds. Prompt observations
    /// no longer disappear on this timer: missing evidence is not resolution.
    /// Remaining wake/refusal kinds require their own recovery reconciliation.
    pub const STALE_REFUSAL_SECONDS: i64 = 180;

    /// Last unresolved observations for Queues, not proof of current prompt state.
    ///
    /// # Errors
    /// Returns an error when the ledger cannot be read or exceeds 256 results.
    pub fn standing_coordinator_refusals(
        &self,
        now: i64,
        grace_seconds: i64,
    ) -> Result<Vec<CoordinatorRefusal>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT refusal.kind, refusal.subject, refusal.worker_id, worker.name,
                    refusal.reason, refusal.first_observed_at, refusal.last_observed_at,
                    refusal.observations
             FROM coordinator_refusals refusal
             LEFT JOIN worker_profiles worker ON worker.id = refusal.worker_id
             WHERE refusal.cleared_at IS NULL
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'task-dispatch:%'
                    OR EXISTS (
                        SELECT 1 FROM task_dispatches dispatch
                        JOIN task_assignments assignment ON assignment.id = dispatch.assignment_id
                        JOIN tasks task ON task.id = dispatch.task_id
                        WHERE dispatch.assignment_id = substr(refusal.subject, 15)
                          AND task.removed_at IS NULL AND task.state IN ('ready', 'active')
                          AND dispatch.state IN ('queued', 'dispatching') AND assignment.released_at IS NULL
                          AND dispatch.worker_id = refusal.worker_id
                          AND assignment.worker_session_id = refusal.session_id
                    ))
               -- A known task's briefing hold is obsolete once its live
               -- assignment no longer has a pending dispatch. Keep unknown
               -- legacy subjects as unresolved rather than guessing recovery.
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'task-brief:%'
                    OR NOT EXISTS (SELECT 1 FROM tasks task WHERE task.id = substr(refusal.subject, 12))
                    OR EXISTS (
                        SELECT 1 FROM task_dispatches dispatch
                        JOIN task_assignments assignment ON assignment.id = dispatch.assignment_id
                        JOIN tasks task ON task.id = dispatch.task_id
                        WHERE task.id = substr(refusal.subject, 12)
                          AND task.removed_at IS NULL AND task.state IN ('ready', 'active')
                          AND dispatch.state IN ('queued', 'dispatching') AND assignment.released_at IS NULL
                          AND (refusal.worker_id IS NULL OR dispatch.worker_id = refusal.worker_id)
                          AND (refusal.session_id IS NULL OR assignment.worker_session_id = refusal.session_id)
                    ))
               -- A known ended session cannot still hold terminal input.
               -- Preserve unbound legacy evidence and non-terminal recovery.
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.session_id IS NULL
                    OR NOT EXISTS (
                        SELECT 1 FROM worker_sessions session
                        WHERE session.session_id = refusal.session_id
                          AND session.ended_at IS NOT NULL
                    ))
               AND ?1 - refusal.first_observed_at >= ?2
               AND (refusal.kind IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR ?1 - refusal.last_observed_at <= ?3)
             ORDER BY refusal.first_observed_at LIMIT 257",
        )?;
        let rows = statement.query_map(
            params![now, grace_seconds, Self::STALE_REFUSAL_SECONDS],
            |row| {
                Ok(CoordinatorRefusal {
                    kind: row.get(0)?,
                    subject: row.get(1)?,
                    worker_id: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|id| WorkerId::from_str(&id).ok()),
                    worker_name: row.get(3)?,
                    reason: row.get(4)?,
                    first_observed_at: row.get(5)?,
                    last_observed_at: row.get(6)?,
                    observations: row.get(7)?,
                })
            },
        )?;
        let refusals = rows.collect::<Result<Vec<_>, _>>()?;
        if refusals.len() > 256 {
            return Err(TaskStoreError::IntegrityFailure(
                "coordinator refusal projection limit exceeded".into(),
            ));
        }
        Ok(refusals)
    }
}
