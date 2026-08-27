use std::{
    collections::{BTreeMap, HashMap, HashSet},
    str::FromStr,
};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, DecisionDeliveryState, DecisionQuestion, DecisionRequest,
    DecisionRequestId, DecisionRequestKind, DecisionRequestState, DecisionUrgency,
    MAX_DECISION_QUESTION_HEADER_BYTES, MAX_DECISION_QUESTION_OPTION_BYTES,
    MAX_DECISION_QUESTION_OPTIONS, MAX_DECISION_QUESTION_TEXT_BYTES, MAX_DECISION_QUESTIONS,
    MAX_DECISION_SUMMARY_BYTES, MIN_DECISION_QUESTION_OPTIONS, TaskId, WorkerId, WorkerSessionId,
};

use super::{
    DECISION_QUESTIONS_SCHEMA_VERSION, DECISION_RESOLUTION_SURFACE_SCHEMA_VERSION,
    DECISION_SUMMARY_SCHEMA_VERSION, TaskStore, TaskStoreError, insert_control_room_event,
};

/// Carries the questions an interview asks and the answers it collects.
///
/// Both default to empty, which is exactly what a ruling holds, so every record
/// written before interviews existed keeps behaving as it did.
///
/// # Errors
/// Returns an error when the step cannot be applied.
/// Carries the one or two sentences saying what the operator is deciding.
///
/// # Errors
/// Returns an error when the step cannot be applied.
pub(super) fn migrate_decision_summary(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let decisions_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'decision_requests')",
        [],
        |row| row.get(0),
    )?;
    let present: bool = decisions_exist
        && transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('decision_requests') WHERE name = 'summary')",
            [],
            |row| row.get(0),
        )?;
    if decisions_exist && !present {
        transaction.execute_batch(
            "ALTER TABLE decision_requests ADD COLUMN summary TEXT NOT NULL DEFAULT '';",
        )?;
    }
    transaction.pragma_update(None, "user_version", DECISION_SUMMARY_SCHEMA_VERSION)
}

pub(super) fn migrate_decision_questions(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let decisions_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'decision_requests')",
        [],
        |row| row.get(0),
    )?;
    if decisions_exist {
        for (column, default) in [("questions", "'[]'"), ("resolution_answers", "'{}'")] {
            let present: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('decision_requests') WHERE name = ?1)",
                [column],
                |row| row.get(0),
            )?;
            if !present {
                transaction.execute_batch(&format!(
                    "ALTER TABLE decision_requests ADD COLUMN {column} TEXT NOT NULL DEFAULT {default};"
                ))?;
            }
        }
    }
    transaction.pragma_update(None, "user_version", DECISION_QUESTIONS_SCHEMA_VERSION)
}

/// Records which surface submitted a resolution.
///
/// A resolution the operator says they did not choose could not be traced,
/// because nothing recorded where the answer came in. A forward step, guarded
/// on the table, because a database older than decisions passes through here.
///
/// # Errors
/// Returns an error when the step cannot be applied.
pub(super) fn migrate_decision_resolution_surface(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let decisions_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'decision_requests')",
        [],
        |row| row.get(0),
    )?;
    let column_exists: bool = decisions_exist
        && transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('decision_requests')
             WHERE name = 'resolution_surface')",
            [],
            |row| row.get(0),
        )?;
    if decisions_exist && !column_exists {
        transaction.execute_batch(
            "ALTER TABLE decision_requests ADD COLUMN resolution_surface TEXT NOT NULL DEFAULT '';",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        DECISION_RESOLUTION_SURFACE_SCHEMA_VERSION,
    )
}

pub const MAX_PENDING_DECISIONS: i64 = 256;
pub const MAX_DECISION_RESULTS: i64 = 200;
const MAX_TITLE_BYTES: usize = 240;
const MAX_DETAIL_BYTES: usize = 10_000;
const MAX_ACTION_BYTES: usize = 80;
const MAX_RESOLUTION_SURFACE_BYTES: usize = 40;
const MAX_ACTIONS: usize = 6;
const MAX_RESOLUTION_NOTE_BYTES: usize = 4_000;
const MAX_DELIVERY_CLAIMS: i64 = 16;
const MAX_DELIVERY_ATTEMPTS: i64 = 3;
const OPERATOR_DISMISS_ACTION: &str = "dismissed";
/// The action recorded when an interview is answered.
///
/// The schema requires every resolved record to name an action, and the audit
/// trail should read as clearly for an interview as for a button. "answered"
/// is that action; the substance is in `resolution_answers`.
pub const INTERVIEW_ANSWERED_ACTION: &str = "answered";
/// The key a ruling's free answer is recorded under.
///
/// A ruling declares no questions, so there is no header to key its answer by.
/// Naming it here keeps one answer shape for both, which means one delivery
/// format and one audit trail rather than a second of each.
pub const OPERATOR_ANSWER_HEADER: &str = "Answer";

/// A worker holding its session until the operator answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HeldForAnswer {
    /// When the oldest unanswered request from this worker was filed.
    pub since: i64,
    /// Whether any of them has passed the deadline its asker set.
    pub overdue: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionDispatch {
    pub decision_id: DecisionRequestId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub action: String,
    pub note: String,
    /// The operator's answers when this was an interview, keyed by question
    /// header. Empty for a ruling.
    pub answers: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionDeliveryFailure {
    Retryable,
    Uncertain,
}
#[derive(Clone, Debug)]
pub struct NewDecisionRequest<'a> {
    pub requesting_worker_id: WorkerId,
    pub task_id: Option<TaskId>,
    pub kind: DecisionRequestKind,
    pub urgency: DecisionUrgency,
    pub title: &'a str,
    /// One or two sentences on what the operator is deciding.
    pub summary: &'a str,
    pub reason: &'a str,
    pub risk: &'a str,
    pub evidence: &'a str,
    pub suggested_action: &'a str,
    pub allowed_actions: &'a [String],
    /// Present makes this an interview rather than a ruling. Empty is a ruling.
    pub questions: &'a [DecisionQuestion],
    pub deadline: Option<i64>,
    /// The exact command this decision would authorise, if approving it should
    /// make that command RUNNABLE.
    ///
    /// Shown to the operator verbatim, and it is the reason this is a field
    /// rather than something inferred from the prose: approving "the one contact
    /// formula-column test" is not approving a regex. A decision that silently
    /// compiled to a permission pattern would trade a visible block for an
    /// invisible grant.
    pub requested_command: Option<&'a str>,
}

/// The one action that turns an approval into a runnable grant.
///
/// EXACT MATCH, never a classifier. Whether a resolution approves something is a
/// judgement about prose; whether it equals this string is not.
///
/// SHORT AND CONSTANT rather than carrying the command, because an action label
/// is capped at eighty bytes and a real command does not fit — the first version
/// of this embedded the command and was refused at resolution. That cap turns
/// out to be the better design: a truncated command in a button would be the
/// worst possible place to read what you are authorising. The command lives in
/// the decision's `requested_command`, where there is room for it whole, and the
/// button binds to the record rather than to a copy of the text.
pub const GRANT_COMMAND_ACTION: &str = "Allow the command shown in this request";

impl TaskStore {
    /// Returns the workers with an unresolved explicit operator decision.
    ///
    /// # Errors
    /// Returns database or persisted-identity failures.
    pub fn workers_awaiting_operator(&self) -> Result<HashSet<WorkerId>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT d.requesting_worker_id
             FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
             WHERE d.state = 'pending'",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|result| -> Result<WorkerId, TaskStoreError> {
                let id = result?;
                Ok(parse_id(&id)?)
            })
            .collect()
    }

    /// Forgets unconfirmed decision deliveries that no longer mean anything.
    ///
    /// An uncertain delivery marks its worker: Swarm wrote an answer and could
    /// not confirm it landed. That mark is worth carrying while the answer
    /// still matters. It stops meaning anything once the decision is resolved
    /// and the session it was written to has ended — there is nothing left to
    /// deliver and no terminal left to check.
    ///
    /// Briefings already had this. Decisions did not, so their marks
    /// accumulated: five of them, every one against a resolved decision on a
    /// dead session, on workers the operator reported had been showing the mark
    /// for days with no open work at all.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn forget_moot_unconfirmed_answers(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let forgotten = transaction.execute(
            "DELETE FROM decision_deliveries
             WHERE state = 'uncertain'
               AND NOT EXISTS (
                   SELECT 1 FROM decision_requests request
                   WHERE request.id = decision_deliveries.decision_id
                     AND request.state = 'pending'
               )
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions session
                   WHERE session.session_id = decision_deliveries.session_id
                     AND session.ended_at IS NULL
               )",
            [],
        )?;
        if forgotten > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(forgotten)
    }

    /// How long each worker has been holding for an operator answer, and
    /// whether that wait has passed the deadline the asker set.
    ///
    /// A worker that files a decision stops and holds its session. The roster
    /// showed how long its terminal had been silent, which for a held worker is
    /// a coincidence rather than the fact: it measures when output stopped, not
    /// how long an answer has been owed. A pinned session with no visible
    /// reason is a worse failure than the guess-the-button problem it came
    /// from.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or holds an invalid ID.
    pub fn workers_holding_for_an_answer(
        &self,
        now: i64,
    ) -> Result<HashMap<WorkerId, HeldForAnswer>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.requesting_worker_id, MIN(d.created_at),
                    MAX(CASE WHEN d.deadline IS NOT NULL AND d.deadline <= ?1 THEN 1 ELSE 0 END)
             FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
             WHERE d.state = 'pending'
             GROUP BY d.requesting_worker_id",
        )?;
        let rows = statement
            .query_map([now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, since, overdue)| Ok((parse_id(&id)?, HeldForAnswer { since, overdue })))
            .collect()
    }

    /// Creates one validated, local-Hive request and emits an inbox event atomically.
    ///
    /// # Errors
    /// Returns a validation, capacity, identity, integrity, or database error.
    pub fn create_decision_request(
        &self,
        request: &NewDecisionRequest<'_>,
    ) -> Result<DecisionRequest, TaskStoreError> {
        validate_new_request(request)?;
        let questions = serde_json::to_string(request.questions)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        // SWARM APPENDS THE GRANT BUTTON, never the caller. If a worker could
        // supply the label that mints a grant, the exact-match check below would
        // be checking a string the worker chose — which is not a check.
        let mut offered = request.allowed_actions.to_vec();
        if request.requested_command.is_some() {
            let label = GRANT_COMMAND_ACTION.to_owned();
            if !offered.contains(&label) {
                offered.push(label);
            }
        }
        let actions = serde_json::to_string(&offered)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let id = DecisionRequestId::new();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let now: i64 = transaction.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        if request.deadline.is_some_and(|deadline| deadline <= now) {
            return Err(TaskStoreError::InvalidDecisionDeadline);
        }
        let pending: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
             WHERE d.state = 'pending'",
            [],
            |row| row.get(0),
        )?;
        if pending >= MAX_PENDING_DECISIONS {
            return Err(TaskStoreError::DecisionInboxFull);
        }
        let inserted = transaction.execute(
            "INSERT INTO decision_requests (
                id, hive_id, requesting_worker_id, task_id, kind, urgency, title, reason,
                risk, evidence, suggested_action, allowed_actions, deadline, questions,
                summary, requested_command
             )
             SELECT ?1, w.hive_id, w.id, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
             FROM worker_profiles w
             JOIN local_hive_identity l ON l.hive_id = w.hive_id AND l.singleton = 1
             WHERE w.id = ?2
               AND (?3 IS NULL OR EXISTS (
                   SELECT 1 FROM tasks t WHERE t.id = ?3 AND t.hive_id = w.hive_id
               ))",
            params![
                id.to_string(),
                request.requesting_worker_id.to_string(),
                request.task_id.map(|value| value.to_string()),
                request.kind.to_string(),
                request.urgency.to_string(),
                request.title,
                request.reason,
                request.risk,
                request.evidence,
                request.suggested_action,
                actions,
                request.deadline,
                questions,
                request.summary,
                request.requested_command,
            ],
        )?;
        if inserted == 0 {
            return Err(TaskStoreError::WorkerNotFound);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        super::notifications::enqueue_decision_notifications(
            &transaction,
            id,
            request.urgency,
            now,
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_decision_request(id)
    }

    /// Returns a decision request by its durable identity.
    ///
    /// # Errors
    /// Returns `DecisionNotFound` or a database/integrity error.
    pub fn get_decision_request(
        &self,
        id: DecisionRequestId,
    ) -> Result<DecisionRequest, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, hive_id, requesting_worker_id, task_id, kind, urgency, title,
                        reason, risk, evidence, suggested_action, allowed_actions, deadline,
                        state, resolution_action, resolution_note, resolved_by_operator_id,
                        resolution_surface, questions, resolution_answers, summary,
                        created_at, updated_at, resolved_at,
                        (SELECT state FROM decision_deliveries WHERE decision_id = decision_requests.id)
                 FROM decision_requests WHERE id = ?1",
                [id.to_string()],
                decision_from_row,
            )
            .optional()?
            .ok_or(TaskStoreError::DecisionNotFound)
    }

    /// Lists the bounded local-Hive inbox with pending and time-sensitive work first.
    ///
    /// # Errors
    /// Returns a database or persisted-data integrity error.
    pub fn list_decision_requests(&self) -> Result<Vec<DecisionRequest>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.id, d.hive_id, d.requesting_worker_id, d.task_id, d.kind, d.urgency, d.title,
                    reason, risk, evidence, suggested_action, allowed_actions, deadline,
                    state, resolution_action, resolution_note, resolved_by_operator_id,
                    resolution_surface, questions, resolution_answers, summary,
                    created_at, updated_at, resolved_at,
                    (SELECT state FROM decision_deliveries WHERE decision_id = d.id)
             FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
             ORDER BY state = 'pending' DESC, urgency = 'time_sensitive' DESC,
                      deadline IS NULL, deadline, created_at DESC, id DESC
             LIMIT ?1",
        )?;
        statement
            .query_map([MAX_DECISION_RESULTS], decision_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Answers an interview and delivers the answers to the worker that asked.
    ///
    /// Every declared question must carry an answer. A worker holding its
    /// session on an interview is waiting for the whole set, and resuming it
    /// with half of one would give it an incomplete picture and no way to ask
    /// the rest without opening a second record.
    ///
    /// An answer is not restricted to the offered options. An answer that
    /// matches none of them is the most informative kind — it is the case the
    /// asker failed to guess, which is the reason interviews exist — so it is
    /// stored as given.
    ///
    /// # Errors
    /// Returns `DecisionNotFound`, `DecisionAlreadyResolved`,
    /// `IncompleteDecisionAnswers`, or a database error.
    pub fn answer_decision_request(
        &self,
        id: DecisionRequestId,
        answers: &BTreeMap<String, Vec<String>>,
        note: &str,
        surface: &str,
    ) -> Result<DecisionRequest, TaskStoreError> {
        if note.len() > MAX_RESOLUTION_NOTE_BYTES || surface.len() > MAX_RESOLUTION_SURFACE_BYTES {
            return Err(TaskStoreError::InvalidDecisionResolution);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, declared): (String, String) = transaction
            .query_row(
                "SELECT d.state, d.questions FROM decision_requests d
                 JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
                 WHERE d.id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::DecisionNotFound)?;
        if state != "pending" {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        let declared: Vec<DecisionQuestion> = serde_json::from_str(&declared)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        if declared.is_empty() {
            // A record with no questions is a ruling, and its buttons are the
            // asker's guesses. When none of them is the operator's answer, the
            // answer is still the operator's to give: the alternative is
            // pressing a wrong button or dismissing, and both lose it.
            //
            // Exactly one free answer, under a reserved key, so a ruling
            // answered in words carries the same shape as an interview and
            // reaches the worker through the same delivery.
            let spoken = answers.get(OPERATOR_ANSWER_HEADER);
            if answers.len() != 1
                || !spoken.is_some_and(|given| given.iter().any(|value| !value.trim().is_empty()))
            {
                return Err(TaskStoreError::InvalidDecisionResolution);
            }
            record_answers(transaction, id, answers, note, surface)?;
            drop(connection);
            return self.get_decision_request(id);
        }
        let answered_every_question = declared.iter().all(|question| {
            answers
                .get(&question.header)
                .is_some_and(|given| given.iter().any(|value| !value.trim().is_empty()))
        });
        if !answered_every_question || answers.len() != declared.len() {
            return Err(TaskStoreError::IncompleteDecisionAnswers);
        }
        record_answers(transaction, id, answers, note, surface)?;
        drop(connection);
        self.get_decision_request(id)
    }

    /// Resolves a pending local-Hive request using one of its declared actions.
    ///
    /// # Errors
    /// Returns an identity, state, validation, integrity, or database error.
    pub fn resolve_decision_request(
        &self,
        id: DecisionRequestId,
        action: &str,
        note: &str,
        surface: &str,
    ) -> Result<DecisionRequest, TaskStoreError> {
        if action.is_empty()
            || action.len() > MAX_ACTION_BYTES
            || note.len() > MAX_RESOLUTION_NOTE_BYTES
            || surface.len() > MAX_RESOLUTION_SURFACE_BYTES
        {
            return Err(TaskStoreError::InvalidDecisionResolution);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, actions, questions): (String, String, String) = transaction
            .query_row(
                "SELECT d.state, d.allowed_actions, d.questions FROM decision_requests d
                 JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
                 WHERE d.id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::DecisionNotFound)?;
        if state != "pending" {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        // An interview offers no actions, so the only resolution this path can
        // carry for one is a dismissal — and a dismissal without a note is the
        // failure this whole shape exists to stop. "Hold, I will deal with it
        // later" and "stop asking me about this" were recorded identically, and
        // the asker had to collapse both into changing nothing.
        if questions != "[]" {
            if action != OPERATOR_DISMISS_ACTION {
                return Err(TaskStoreError::InvalidDecisionResolution);
            }
            if note.trim().is_empty() {
                return Err(TaskStoreError::DismissedInterviewNeedsReason);
            }
        }
        let actions: Vec<String> = serde_json::from_str(&actions)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        // The model controls the proposed actions, but it must never control
        // whether the operator can decline the request altogether. Dismissal
        // records a durable resolution and reports it back to the requester;
        // it does not execute any proposed action.
        if action != OPERATOR_DISMISS_ACTION && !actions.iter().any(|candidate| candidate == action)
        {
            return Err(TaskStoreError::InvalidDecisionResolution);
        }
        let updated = transaction.execute(
            "UPDATE decision_requests SET
                 state = 'resolved', resolution_action = ?2, resolution_note = ?3,
                 resolution_surface = ?4,
                 resolved_by_operator_id = (
                     SELECT h.operator_id FROM local_hive_identity l
                     JOIN hives h ON h.id = l.hive_id WHERE l.singleton = 1
                 ),
                 resolved_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?1 AND state = 'pending'",
            params![id.to_string(), action, note, surface],
        )?;
        if updated != 1 {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        // The grant, and the whole safety of it is that this is an EQUALITY
        // check rather than a reading of the operator's prose. It fires only
        // when they pressed the button naming the exact command, so what they
        // authorised and what becomes runnable cannot drift apart.
        //
        // A decision with no task grants nothing: a grant dies with its task,
        // and one with nothing to die with would be a standing rule.
        let requested: Option<String> = transaction
            .query_row(
                "SELECT requested_command FROM decision_requests WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if requested.is_some() && action == GRANT_COMMAND_ACTION {
            // The label is rebuilt from the STORED command and compared to what
            // the operator chose. An earlier version of this compared
            // resolution_action to the action being written, which is the same
            // string by construction and would therefore have granted on ANY
            // resolution -- including a refusal.
            //
            // A decision with no task grants nothing: a grant dies with its
            // task, and one with nothing to die with is a standing rule.
            transaction.execute(
                "INSERT INTO decision_command_grants (decision_id, task_id, worker_id, command)
                 SELECT d.id, d.task_id, d.requesting_worker_id, d.requested_command
                 FROM decision_requests d
                 WHERE d.id = ?1 AND d.task_id IS NOT NULL",
                [id.to_string()],
            )?;
        }
        transaction.execute(
            "INSERT INTO decision_deliveries (decision_id, worker_id, state)
             SELECT id, requesting_worker_id, 'queued' FROM decision_requests WHERE id = ?1",
            [id.to_string()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_decision_request(id)
    }

    /// The commands this worker may run because the operator approved them.
    ///
    /// Unconsumed grants only, and only while the task they were created for is
    /// still on the board — a grant that outlives its task is the standing rule
    /// the operator explicitly refused, wearing a costume.
    ///
    /// # Errors
    /// Returns an error when the query fails.
    pub fn live_command_grants(&self, worker_id: WorkerId) -> Result<Vec<String>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT g.command
             FROM decision_command_grants g
             JOIN tasks t ON t.id = g.task_id
             WHERE g.worker_id = ?1
               AND g.consumed_at IS NULL
               AND t.removed_at IS NULL
               AND t.state != 'completed'
             ORDER BY g.created_at",
        )?;
        let commands = statement
            .query_map([worker_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(commands)
    }

    /// Marks every live grant for a worker as spent.
    ///
    /// Called when the session those grants were written into ends. This is what
    /// "one use" can honestly mean here: the classifier reads a settings file at
    /// process start and reports nothing back, so exactly-once cannot be enforced
    /// AT the classifier. What is enforced is that a grant is offered to one
    /// session and never a second.
    ///
    /// # Errors
    /// Returns an error when the update fails.
    pub fn consume_command_grants(&self, worker_id: WorkerId) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        let spent = connection.execute(
            "UPDATE decision_command_grants SET consumed_at = unixepoch()
             WHERE worker_id = ?1 AND consumed_at IS NULL",
            [worker_id.to_string()],
        )?;
        Ok(spent)
    }

    /// Atomically claims a bounded batch whose worker is running and not operator-engaged.
    ///
    /// # Errors
    /// Returns a persistence or integrity error.
    pub fn claim_decision_deliveries(
        &self,
        now: i64,
    ) -> Result<Vec<DecisionDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT dd.decision_id, dd.worker_id, ws.session_id,
                        d.resolution_action, d.resolution_note, d.resolution_answers
                 FROM decision_deliveries dd
                 JOIN decision_requests d ON d.id = dd.decision_id
                 JOIN worker_sessions ws ON ws.worker_id = dd.worker_id AND ws.ended_at IS NULL
                 WHERE dd.state = 'queued'
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements e
                       WHERE e.worker_id = dd.worker_id AND e.expires_at > ?1
                   )
                 ORDER BY dd.updated_at, dd.decision_id
                 LIMIT ?2",
            )?;
            statement
                .query_map(params![now, MAX_DELIVERY_CLAIMS], |row| {
                    Ok(DecisionDispatch {
                        decision_id: parse_id(&row.get::<_, String>(0)?)?,
                        worker_id: parse_id(&row.get::<_, String>(1)?)?,
                        session_id: parse_id(&row.get::<_, String>(2)?)?,
                        action: row.get(3)?,
                        note: row.get(4)?,
                        answers: serde_json::from_str(&row.get::<_, String>(5)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for delivery in &candidates {
            let updated = transaction.execute(
                "UPDATE decision_deliveries SET state = 'dispatching', session_id = ?2,
                     attempts = attempts + 1, attempted_at = ?3, updated_at = ?3
                 WHERE decision_id = ?1 AND state = 'queued' AND attempts < ?4",
                params![
                    delivery.decision_id.to_string(),
                    delivery.session_id.to_string(),
                    now,
                    MAX_DELIVERY_ATTEMPTS,
                ],
            )?;
            if updated != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "decision delivery claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Records an acknowledged delivery.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn complete_decision_delivery(
        &self,
        id: DecisionRequestId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_decision_delivery(id, now, None)
    }

    /// Records a definitive rejection for retry or a crash-ambiguous result as uncertain.
    ///
    /// # Errors
    /// Returns a persistence or integrity error.
    pub fn fail_decision_delivery(
        &self,
        id: DecisionRequestId,
        now: i64,
        failure: DecisionDeliveryFailure,
    ) -> Result<bool, TaskStoreError> {
        self.finish_decision_delivery(id, now, Some(failure))
    }

    /// Returns a claimed delivery to its durable queue without consuming an
    /// attempt when the provider is waiting for operator input.
    ///
    /// # Errors
    /// Returns a persistence or integrity error.
    pub fn defer_decision_delivery(
        &self,
        id: DecisionRequestId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE decision_deliveries
             SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
             WHERE decision_id = ?1 AND state = 'dispatching'",
            params![id.to_string(), now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    fn finish_decision_delivery(
        &self,
        id: DecisionRequestId,
        now: i64,
        failure: Option<DecisionDeliveryFailure>,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, delivered_at) = match failure {
            None => ("delivered", Some(now)),
            Some(DecisionDeliveryFailure::Uncertain) => ("uncertain", None),
            Some(DecisionDeliveryFailure::Retryable) => {
                let attempts: i64 = transaction.query_row(
                    "SELECT attempts FROM decision_deliveries
                     WHERE decision_id = ?1 AND state = 'dispatching'",
                    [id.to_string()],
                    |row| row.get(0),
                )?;
                (
                    if attempts >= MAX_DELIVERY_ATTEMPTS {
                        "uncertain"
                    } else {
                        "queued"
                    },
                    None,
                )
            }
        };
        let changed = transaction.execute(
            "UPDATE decision_deliveries SET state = ?2, delivered_at = ?3, updated_at = ?4
             WHERE decision_id = ?1 AND state = 'dispatching'",
            params![id.to_string(), state, delivered_at, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Converts crash-interrupted dispatches to an explicit non-retrying state.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_decision_deliveries(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE decision_deliveries SET state = 'uncertain', updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?;
        if changed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}

/// Writes an answered resolution and queues it back to the asking worker.
fn record_answers(
    transaction: rusqlite::Transaction<'_>,
    id: DecisionRequestId,
    answers: &BTreeMap<String, Vec<String>>,
    note: &str,
    surface: &str,
) -> Result<(), TaskStoreError> {
    let recorded = serde_json::to_string(answers)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    let updated = transaction.execute(
        "UPDATE decision_requests SET
             state = 'resolved', resolution_action = 'answered',
             resolution_answers = ?2, resolution_note = ?3,
             resolution_surface = ?4,
             resolved_by_operator_id = (
                 SELECT h.operator_id FROM local_hive_identity l
                 JOIN hives h ON h.id = l.hive_id WHERE l.singleton = 1
             ),
             resolved_at = unixepoch(), updated_at = unixepoch()
         WHERE id = ?1 AND state = 'pending'",
        params![id.to_string(), recorded, note, surface],
    )?;
    if updated != 1 {
        return Err(TaskStoreError::DecisionAlreadyResolved);
    }
    transaction.execute(
        "INSERT INTO decision_deliveries (decision_id, worker_id, state)
         SELECT id, requesting_worker_id, 'queued' FROM decision_requests WHERE id = ?1",
        [id.to_string()],
    )?;
    insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
    transaction.commit()?;
    Ok(())
}

fn validate_new_request(request: &NewDecisionRequest<'_>) -> Result<(), TaskStoreError> {
    let content = [
        request.title,
        request.reason,
        request.risk,
        request.evidence,
        request.suggested_action,
    ];
    // The summary is what the operator reads first and, often, only. It is
    // required and tightly bounded because reason, risk and evidence are each
    // capped at ten thousand characters and routinely run to thousands — about
    // five thousand characters to read before a decision can be made. A cap
    // that generous invites the argument to stand in for the ask.
    if request.summary.trim().is_empty() || request.summary.len() > MAX_DECISION_SUMMARY_BYTES {
        return Err(TaskStoreError::InvalidDecisionSummary);
    }
    if request.title.is_empty()
        || request.reason.is_empty()
        || request.suggested_action.is_empty()
        || request.title.len() > MAX_TITLE_BYTES
        || content[1..]
            .iter()
            .any(|value| value.len() > MAX_DETAIL_BYTES)
    {
        return Err(TaskStoreError::InvalidDecisionContent);
    }
    // A record is either a ruling or an interview, never both. Permitting both
    // invites a record whose button says one thing and whose answers say
    // another, with no defined precedence between them.
    if !request.questions.is_empty() {
        if !request.allowed_actions.is_empty() {
            return Err(TaskStoreError::InvalidDecisionQuestions);
        }
        return validate_questions(request.questions);
    }
    let mut unique = HashSet::new();
    if request.allowed_actions.is_empty()
        || request.allowed_actions.len() > MAX_ACTIONS
        || request.allowed_actions.iter().any(|action| {
            action.is_empty() || action.len() > MAX_ACTION_BYTES || !unique.insert(action.as_str())
        })
    {
        return Err(TaskStoreError::InvalidDecisionActions);
    }
    Ok(())
}

/// Bounds an interview so it stays an instrument rather than a questionnaire.
///
/// The caps mirror `AskUserQuestion`, which is what the operator was
/// interviewed with when this was specified. Headers must be unique because
/// they key the answers: two questions sharing one would lose an answer
/// silently.
fn validate_questions(questions: &[DecisionQuestion]) -> Result<(), TaskStoreError> {
    if questions.len() > MAX_DECISION_QUESTIONS {
        return Err(TaskStoreError::InvalidDecisionQuestions);
    }
    let mut headers = HashSet::new();
    for question in questions {
        let mut options = HashSet::new();
        if question.header.trim().is_empty()
            || question.header.len() > MAX_DECISION_QUESTION_HEADER_BYTES
            || !headers.insert(question.header.as_str())
            || question.question.trim().is_empty()
            || question.question.len() > MAX_DECISION_QUESTION_TEXT_BYTES
            || question.options.len() < MIN_DECISION_QUESTION_OPTIONS
            || question.options.len() > MAX_DECISION_QUESTION_OPTIONS
            || question.options.iter().any(|option| {
                option.trim().is_empty()
                    || option.len() > MAX_DECISION_QUESTION_OPTION_BYTES
                    || !options.insert(option.as_str())
            })
        {
            return Err(TaskStoreError::InvalidDecisionQuestions);
        }
    }
    Ok(())
}

fn decision_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionRequest> {
    let actions =
        serde_json::from_str::<Vec<String>>(&row.get::<_, String>(11)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(DecisionRequest {
        id: parse_id(&row.get::<_, String>(0)?)?,
        hive_id: parse_id(&row.get::<_, String>(1)?)?,
        requesting_worker_id: parse_id(&row.get::<_, String>(2)?)?,
        task_id: row
            .get::<_, Option<String>>(3)?
            .map(|value| parse_id(&value))
            .transpose()?,
        kind: DecisionRequestKind::from_str(&row.get::<_, String>(4)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        urgency: DecisionUrgency::from_str(&row.get::<_, String>(5)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        title: row.get(6)?,
        reason: row.get(7)?,
        risk: row.get(8)?,
        evidence: row.get(9)?,
        suggested_action: row.get(10)?,
        allowed_actions: actions,
        deadline: row.get(12)?,
        state: DecisionRequestState::from_str(&row.get::<_, String>(13)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        resolution_action: row.get(14)?,
        resolution_note: row.get(15)?,
        resolved_by_operator_id: row
            .get::<_, Option<String>>(16)?
            .map(|value| parse_id(&value))
            .transpose()?,
        resolution_surface: row.get(17)?,
        questions: serde_json::from_str(&row.get::<_, String>(18)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        resolution_answers: serde_json::from_str(&row.get::<_, String>(19)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        summary: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
        resolved_at: row.get(23)?,
        delivery_state: row
            .get::<_, Option<String>>(24)?
            .map(|value| DecisionDeliveryState::from_str(&value))
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn parse_id<T: FromStr>(value: &str) -> rusqlite::Result<T> {
    T::from_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{PresenceDeviceId, ProviderKind, TaskPriority};

    /// A grant exists only when the operator pressed the button naming the command.
    ///
    /// The whole safety of this rests on an EQUALITY check rather than a reading
    /// of the operator's prose. Approving "the one contact formula-column test"
    /// is not approving a regex, and a decision that silently compiled to a
    /// permission pattern would trade a visible block for an invisible grant.
    ///
    /// The refusal case is the one that matters. An earlier version of this
    /// compared `resolution_action` against the action being written — the same
    /// string by construction — so it would have granted on ANY resolution,
    /// including a refusal. That is the shape of bug this test exists to catch.
    #[test]
    fn a_command_grant_needs_the_operator_to_press_the_button_that_names_it() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let task = store
            .create_task("Create the formula column", "/workspace/d365")
            .unwrap();
        let command =
            "curl -sS -X POST https://example.crm.dynamics.com/api/data/v9.2/AttributeMetadata";
        let refusals = vec!["Do not run it".to_owned()];

        // Refused: the operator picked something else.
        let refused = store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(task.id),
                kind: DecisionRequestKind::Approval,
                urgency: DecisionUrgency::Normal,
                title: "Create one formula column?",
                summary: "One metadata POST against contact.",
                reason: "The classifier denied it and the operator approved the action.",
                risk: "",
                evidence: "",
                suggested_action: "Do not run it",
                allowed_actions: &refusals,
                questions: &[],
                deadline: None,
                requested_command: Some(command),
            })
            .unwrap();
        assert!(
            refused
                .allowed_actions
                .iter()
                .any(|action| action == GRANT_COMMAND_ACTION),
            "swarm offers the grant button itself: {:?}",
            refused.allowed_actions
        );
        store
            .resolve_decision_request(refused.id, "Do not run it", "", "inbox")
            .unwrap();
        assert!(
            store.live_command_grants(queen.id).unwrap().is_empty(),
            "a refusal grants nothing"
        );

        // Approved by pressing the button that names the command.
        let approved = store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(task.id),
                kind: DecisionRequestKind::Approval,
                urgency: DecisionUrgency::Normal,
                title: "Create one formula column?",
                summary: "One metadata POST against contact.",
                reason: "The classifier denied it and the operator approved the action.",
                risk: "",
                evidence: "",
                suggested_action: "Do not run it",
                allowed_actions: &refusals,
                questions: &[],
                deadline: None,
                requested_command: Some(command),
            })
            .unwrap();
        store
            .resolve_decision_request(approved.id, GRANT_COMMAND_ACTION, "", "inbox")
            .unwrap();
        assert_eq!(
            store.live_command_grants(queen.id).unwrap(),
            vec![command.to_owned()],
            "the approved command, and only that command"
        );

        // And it dies with the work rather than standing.
        store.consume_command_grants(queen.id).unwrap();
        assert!(
            store.live_command_grants(queen.id).unwrap().is_empty(),
            "a spent grant is not offered to a second session"
        );
    }

    fn request(worker_id: WorkerId, actions: &[String]) -> NewDecisionRequest<'_> {
        NewDecisionRequest {
            requesting_worker_id: worker_id,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency: DecisionUrgency::Normal,
            title: "Choose the rollout window",
            summary: "Whether to proceed, and what it costs if we do not.",
            reason: "The change is ready but needs operator timing.",
            risk: "An active user could see a brief reconnect.",
            evidence: "All automated checks passed.",
            suggested_action: "Deploy after the current session.",
            allowed_actions: actions,
            questions: &[],
            deadline: None,
            requested_command: None,
        }
    }

    #[test]
    fn decision_lifecycle_is_typed_atomic_and_audited() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let actions = vec!["deploy_now".into(), "wait".into()];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();
        assert_eq!(created.state, DecisionRequestState::Pending);
        assert_eq!(created.allowed_actions, actions);
        assert!(matches!(
            store.resolve_decision_request(created.id, "other", "", "test"),
            Err(TaskStoreError::InvalidDecisionResolution)
        ));

        let resolved = store
            .resolve_decision_request(created.id, "wait", "Operator is still testing.", "test")
            .unwrap();
        assert_eq!(resolved.state, DecisionRequestState::Resolved);
        assert_eq!(resolved.resolution_action.as_deref(), Some("wait"));
        assert!(resolved.resolved_by_operator_id.is_some());
        assert!(matches!(
            store.resolve_decision_request(created.id, "wait", "again", "test"),
            Err(TaskStoreError::DecisionAlreadyResolved)
        ));
        let events = store.list_control_room_events(0).unwrap().events;
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == ControlRoomEventKind::DecisionsChanged)
                .count(),
            2
        );
    }

    #[test]
    fn operator_can_dismiss_a_request_without_executing_a_proposed_action() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let actions = vec!["deploy_now".into(), "wait".into()];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        let resolved = store
            .resolve_decision_request(
                created.id,
                OPERATOR_DISMISS_ACTION,
                "The queue changed; review current work again.",
                "test",
            )
            .unwrap();

        assert_eq!(resolved.state, DecisionRequestState::Resolved);
        assert_eq!(
            resolved.resolution_action.as_deref(),
            Some(OPERATOR_DISMISS_ACTION)
        );
        assert_eq!(resolved.delivery_state, Some(DecisionDeliveryState::Queued));
    }

    fn interview(headers: &[(&str, &[&str])]) -> Vec<DecisionQuestion> {
        headers
            .iter()
            .map(|(header, options)| DecisionQuestion {
                header: (*header).to_owned(),
                question: format!("What about {header}?"),
                options: options.iter().map(|o| (*o).to_owned()).collect(),
                multi_select: false,
            })
            .collect()
    }

    #[test]
    fn an_interview_collects_answers_including_one_no_option_offered() {
        // The answer that matches nothing offered is the case the asker failed
        // to guess, which is the reason interviews exist. It must survive to
        // the asker exactly as the operator gave it.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let questions = interview(&[
            ("Scope", &["This repo only", "Every repo"]),
            ("Timing", &["Now", "After the release"]),
        ]);
        let created = store
            .create_decision_request(&NewDecisionRequest {
                allowed_actions: &[],
                questions: &questions,
                ..request(queen.id, &[])
            })
            .unwrap();
        assert_eq!(created.questions.len(), 2);

        let mut answers = BTreeMap::new();
        answers.insert("Scope".to_owned(), vec!["This repo only".to_owned()]);
        answers.insert(
            "Timing".to_owned(),
            vec!["Wait until the Jira mapping is fixed".to_owned()],
        );
        let resolved = store
            .answer_decision_request(created.id, &answers, "Mapping first.", "inbox_interview")
            .unwrap();

        assert_eq!(resolved.state, DecisionRequestState::Resolved);
        assert_eq!(
            resolved.resolution_answers.get("Timing").unwrap(),
            &vec!["Wait until the Jira mapping is fixed".to_owned()],
            "an answer matching no offered option must survive unmodified"
        );
        assert_eq!(
            resolved.resolution_action.as_deref(),
            Some(INTERVIEW_ANSWERED_ACTION),
            "the audit trail must name the outcome as clearly as a button does"
        );
        assert_eq!(resolved.resolution_note, "Mapping first.");
        assert_eq!(resolved.resolution_surface, "inbox_interview");
        // Delivered back to the asker the same way a ruling is.
        assert_eq!(resolved.delivery_state, Some(DecisionDeliveryState::Queued));
    }

    #[test]
    fn a_held_worker_reports_how_long_an_answer_has_been_owed() {
        // Spec 3.2: a held session must be visible as held, and for how long.
        // The roster showed terminal silence, which for a held worker measures
        // when output stopped rather than how long the operator has owed an
        // answer. A pinned session with no visible reason is a worse failure
        // than the guess-the-button problem it came from.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["ship".to_owned()];

        assert!(
            store
                .workers_holding_for_an_answer(1_000)
                .unwrap()
                .is_empty()
        );

        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        let held = store.workers_holding_for_an_answer(1_000).unwrap();
        let queen_hold = held.get(&queen.id).expect("queen is holding");
        assert_eq!(queen_hold.since, created.created_at);
        assert!(!queen_hold.overdue, "no deadline was set");

        // Answering releases the hold.
        store
            .resolve_decision_request(created.id, "ship", "", "test")
            .unwrap();
        assert!(
            store
                .workers_holding_for_an_answer(1_000)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_wait_past_its_deadline_is_reported_as_overdue_and_keeps_holding() {
        // Operator ruling 2026-08-20: escalate, keep holding. Never invent an
        // answer, and never quietly break the hard block — but never stay
        // silent about it either.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["ship".to_owned()];
        let created = store
            .create_decision_request(&NewDecisionRequest {
                // Creation rejects a deadline already in the past, so this is a
                // real future one and the clock is moved past it by the query.
                deadline: Some(4_000_000_000),
                ..request(queen.id, &actions)
            })
            .unwrap();

        let before = store.workers_holding_for_an_answer(3_999_999_999).unwrap();
        assert!(!before.get(&queen.id).unwrap().overdue);

        let after = store.workers_holding_for_an_answer(4_000_000_001).unwrap();
        let overdue = after.get(&queen.id).expect("still holding");
        assert!(overdue.overdue);
        assert_eq!(
            store.get_decision_request(created.id).unwrap().state,
            DecisionRequestState::Pending,
            "an overdue request is escalated, not resolved on the operator's behalf"
        );
    }

    #[test]
    fn a_request_must_say_what_the_operator_is_deciding_and_say_it_briefly() {
        // Raised as: the assessment is way too long and does not give a concise
        // analysis of what is being decided. Measured on the live inbox, a
        // single request ran to roughly five thousand characters across reason,
        // risk and evidence — each of which is capped at ten thousand. A cap
        // that generous lets the argument stand in for the ask.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["ship".to_owned()];

        assert!(matches!(
            store.create_decision_request(&NewDecisionRequest {
                summary: "   ",
                ..request(queen.id, &actions)
            }),
            Err(TaskStoreError::InvalidDecisionSummary)
        ));
        assert!(matches!(
            store.create_decision_request(&NewDecisionRequest {
                summary: &"x".repeat(MAX_DECISION_SUMMARY_BYTES + 1),
                ..request(queen.id, &actions)
            }),
            Err(TaskStoreError::InvalidDecisionSummary)
        ));

        let created = store
            .create_decision_request(&NewDecisionRequest {
                summary: "Whether to ship tonight or wait for the mapping fix.",
                ..request(queen.id, &actions)
            })
            .unwrap();
        assert_eq!(
            created.summary,
            "Whether to ship tonight or wait for the mapping fix."
        );
    }

    #[test]
    fn a_ruling_can_be_answered_in_the_operators_own_words() {
        // Observed 2026-08-20: a request offered three buttons and the operator
        // wanted a fourth thing entirely — "add it to the Play Store itself via
        // the browser extension". Pressing a wrong button or dismissing were
        // the only ways out, and both lose the answer.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = [
            "Install it yourself".to_owned(),
            "Route it elsewhere".to_owned(),
        ];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        let mut spoken = BTreeMap::new();
        spoken.insert(
            OPERATOR_ANSWER_HEADER.to_owned(),
            vec!["Add it to the Play Store yourself, using the browser extension.".to_owned()],
        );
        let resolved = store
            .answer_decision_request(created.id, &spoken, "", "inbox_answer")
            .unwrap();

        assert_eq!(resolved.state, DecisionRequestState::Resolved);
        assert_eq!(
            resolved.resolution_action.as_deref(),
            Some(INTERVIEW_ANSWERED_ACTION)
        );
        assert_eq!(
            resolved
                .resolution_answers
                .get(OPERATOR_ANSWER_HEADER)
                .unwrap(),
            &vec!["Add it to the Play Store yourself, using the browser extension.".to_owned()]
        );
        // Reaches the asker the same way any other resolution does.
        assert_eq!(resolved.delivery_state, Some(DecisionDeliveryState::Queued));
    }

    #[test]
    fn a_ruling_answered_in_words_takes_one_answer_and_not_a_questionnaire() {
        // A ruling declares no questions, so there is nothing to key several
        // answers by. Accepting them would invent a shape nobody declared.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["ship".to_owned()];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        let mut wrong_key = BTreeMap::new();
        wrong_key.insert("Scope".to_owned(), vec!["everything".to_owned()]);
        assert!(matches!(
            store.answer_decision_request(created.id, &wrong_key, "", "test"),
            Err(TaskStoreError::InvalidDecisionResolution)
        ));

        let mut blank = BTreeMap::new();
        blank.insert(OPERATOR_ANSWER_HEADER.to_owned(), vec!["  ".to_owned()]);
        assert!(matches!(
            store.answer_decision_request(created.id, &blank, "", "test"),
            Err(TaskStoreError::InvalidDecisionResolution)
        ));

        assert_eq!(
            store.get_decision_request(created.id).unwrap().state,
            DecisionRequestState::Pending
        );
    }

    #[test]
    fn an_interview_is_not_answered_until_every_question_is() {
        // The asking worker holds its session for the whole set. Resuming it on
        // half an answer gives it an incomplete picture and no way to ask the
        // rest without opening a second record.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let questions = interview(&[("Scope", &["One", "All"]), ("Timing", &["Now", "Later"])]);
        let created = store
            .create_decision_request(&NewDecisionRequest {
                allowed_actions: &[],
                questions: &questions,
                ..request(queen.id, &[])
            })
            .unwrap();

        let mut partial = BTreeMap::new();
        partial.insert("Scope".to_owned(), vec!["One".to_owned()]);
        assert!(matches!(
            store.answer_decision_request(created.id, &partial, "", "test"),
            Err(TaskStoreError::IncompleteDecisionAnswers)
        ));

        // Blank is not an answer either.
        let mut blank = partial.clone();
        blank.insert("Timing".to_owned(), vec!["   ".to_owned()]);
        assert!(matches!(
            store.answer_decision_request(created.id, &blank, "", "test"),
            Err(TaskStoreError::IncompleteDecisionAnswers)
        ));
        assert_eq!(
            store.get_decision_request(created.id).unwrap().state,
            DecisionRequestState::Pending
        );
    }

    #[test]
    fn a_record_is_a_ruling_or_an_interview_and_never_both() {
        // Both would allow a record whose button says one thing and whose
        // answers say another, with no defined precedence.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["ship".to_owned()];
        assert!(matches!(
            store.create_decision_request(&NewDecisionRequest {
                allowed_actions: &actions,
                questions: &interview(&[("Scope", &["One", "All"])]),
                ..request(queen.id, &actions)
            }),
            Err(TaskStoreError::InvalidDecisionQuestions)
        ));
    }

    #[test]
    fn an_interview_is_bounded_so_it_stays_an_instrument() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let too_many = interview(&[
            ("A", &["1", "2"]),
            ("B", &["1", "2"]),
            ("C", &["1", "2"]),
            ("D", &["1", "2"]),
            ("E", &["1", "2"]),
        ]);
        let one_option = interview(&[("A", &["only"])]);
        let duplicate_headers = vec![
            interview(&[("A", &["1", "2"])]).remove(0),
            interview(&[("A", &["3", "4"])]).remove(0),
        ];
        for bad in [too_many, one_option, duplicate_headers] {
            assert!(matches!(
                store.create_decision_request(&NewDecisionRequest {
                    allowed_actions: &[],
                    questions: &bad,
                    ..request(queen.id, &[])
                }),
                Err(TaskStoreError::InvalidDecisionQuestions)
            ));
        }
    }

    #[test]
    fn dismissing_an_interview_needs_a_reason_the_asker_can_act_on() {
        // The recorded failure: dismissed with an empty note, so "hold, I will
        // deal with it later" and "stop asking me about this" were stored
        // identically and the asker had to collapse both into changing nothing.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let created = store
            .create_decision_request(&NewDecisionRequest {
                allowed_actions: &[],
                questions: &interview(&[("Scope", &["One", "All"])]),
                ..request(queen.id, &[])
            })
            .unwrap();

        assert!(matches!(
            store.resolve_decision_request(created.id, OPERATOR_DISMISS_ACTION, "", "test"),
            Err(TaskStoreError::DismissedInterviewNeedsReason)
        ));
        // An interview has no buttons, so no action can resolve one either.
        assert!(matches!(
            store.resolve_decision_request(created.id, "ship", "because", "test"),
            Err(TaskStoreError::InvalidDecisionResolution)
        ));

        let dismissed = store
            .resolve_decision_request(
                created.id,
                OPERATOR_DISMISS_ACTION,
                "Holding until the mapping is fixed; ask again after.",
                "test",
            )
            .unwrap();
        assert_eq!(dismissed.state, DecisionRequestState::Resolved);
        assert!(dismissed.resolution_note.contains("Holding"));
    }

    #[test]
    fn a_ruling_is_untouched_by_interviews_existing() {
        // A record without questions must behave exactly as it did before,
        // including that answers are not how it gets resolved.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["continue".to_owned(), "hold".to_owned()];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        assert!(created.questions.is_empty());
        assert!(created.resolution_answers.is_empty());
        let mut answers = BTreeMap::new();
        answers.insert("Scope".to_owned(), vec!["One".to_owned()]);
        assert!(matches!(
            store.answer_decision_request(created.id, &answers, "", "test"),
            Err(TaskStoreError::InvalidDecisionResolution)
        ));

        let resolved = store
            .resolve_decision_request(created.id, "hold", "", "test")
            .unwrap();
        assert_eq!(resolved.resolution_action.as_deref(), Some("hold"));
        assert!(resolved.resolution_answers.is_empty());
    }

    #[test]
    fn a_resolution_records_where_it_came_from() {
        // An operator reported a decision recorded with an action they did not
        // choose, and nothing said where the answer arrived from, so it could
        // not be traced to a surface.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["continue".to_owned(), "hold".to_owned()];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        let resolved = store
            .resolve_decision_request(created.id, "hold", "", "inbox_action")
            .unwrap();

        assert_eq!(resolved.resolution_surface, "inbox_action");
        assert_eq!(
            store
                .get_decision_request(created.id)
                .unwrap()
                .resolution_surface,
            "inbox_action"
        );
    }

    #[test]
    fn a_resolution_from_an_unnamed_surface_is_still_accepted() {
        // Recording where an answer came from is for diagnosis. A client that
        // says nothing must still be able to answer, or the record becomes a
        // gate on resolving work.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let actions = ["continue".to_owned(), "hold".to_owned()];
        let created = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();

        let resolved = store
            .resolve_decision_request(created.id, "hold", "", "")
            .unwrap();

        assert_eq!(resolved.resolution_surface, "");
    }

    #[test]
    fn awaiting_operator_workers_follow_only_pending_decisions() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let actions = vec!["continue".into(), "stop".into()];
        let decision = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();
        assert_eq!(
            store.workers_awaiting_operator().unwrap(),
            HashSet::from([queen.id])
        );

        store
            .resolve_decision_request(decision.id, "continue", "", "test")
            .unwrap();
        assert!(store.workers_awaiting_operator().unwrap().is_empty());
    }

    #[test]
    fn decisions_require_real_same_hive_workers_tasks_and_bounded_actions() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let task = store
            .create_task_with_details("Release", "", TaskPriority::Normal, "/workspace/queen")
            .unwrap();
        let actions = vec!["approve".into(), "decline".into()];
        let mut valid = request(queen.id, &actions);
        valid.task_id = Some(task.id);
        assert_eq!(
            store.create_decision_request(&valid).unwrap().task_id,
            Some(task.id)
        );

        let unknown = request(WorkerId::new(), &actions);
        assert!(matches!(
            store.create_decision_request(&unknown),
            Err(TaskStoreError::WorkerNotFound)
        ));
        let duplicate_actions = vec!["approve".into(), "approve".into()];
        let invalid = request(queen.id, &duplicate_actions);
        assert!(matches!(
            store.create_decision_request(&invalid),
            Err(TaskStoreError::InvalidDecisionActions)
        ));

        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let mut worker_request = request(worker.id, &actions);
        worker_request.urgency = DecisionUrgency::TimeSensitive;
        store.create_decision_request(&worker_request).unwrap();
        let inbox = store.list_decision_requests().unwrap();
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox[0].urgency, DecisionUrgency::TimeSensitive);
    }

    #[test]
    fn resolved_outcomes_wait_for_engagement_then_deliver_to_the_active_session() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        let actions = vec!["continue".into(), "stop".into()];
        let decision = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();
        let resolved = store
            .resolve_decision_request(decision.id, "continue", "Ship after tests.", "test")
            .unwrap();
        assert_eq!(resolved.delivery_state, Some(DecisionDeliveryState::Queued));

        store
            .renew_worker_engagement(session, Some(PresenceDeviceId::new()), 100, 300)
            .unwrap();
        assert!(store.claim_decision_deliveries(101).unwrap().is_empty());
        let deliveries = store.claim_decision_deliveries(401).unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].session_id, session);
        assert_eq!(deliveries[0].action, "continue");
        assert_eq!(deliveries[0].note, "Ship after tests.");
        assert_eq!(
            store
                .get_decision_request(decision.id)
                .unwrap()
                .delivery_state,
            Some(DecisionDeliveryState::Dispatching)
        );

        assert!(store.complete_decision_delivery(decision.id, 402).unwrap());
        assert_eq!(
            store
                .get_decision_request(decision.id)
                .unwrap()
                .delivery_state,
            Some(DecisionDeliveryState::Delivered)
        );
    }

    #[test]
    fn crash_ambiguity_never_auto_retries_a_terminal_injection() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        let actions = vec!["continue".into()];
        let decision = store
            .create_decision_request(&request(queen.id, &actions))
            .unwrap();
        store
            .resolve_decision_request(decision.id, "continue", "", "test")
            .unwrap();
        assert_eq!(store.claim_decision_deliveries(100).unwrap().len(), 1);
        assert_eq!(store.recover_inflight_decision_deliveries().unwrap(), 1);
        assert!(store.claim_decision_deliveries(101).unwrap().is_empty());
        assert_eq!(
            store
                .get_decision_request(decision.id)
                .unwrap()
                .delivery_state,
            Some(DecisionDeliveryState::Uncertain)
        );
    }

    /// The operator, days apart: "Public Website shows a (!) saying Swarm wrote
    /// a briefing to this worker and could not confirm it landed and has been
    /// that way for ages. It has no open tasks. Same with Sculpt Studio."
    ///
    /// Briefings were given a way to stop meaning something. Answers were not,
    /// so their marks accumulated — five of them in the live database, every
    /// one against a decision already resolved on a session already ended.
    #[test]
    fn an_answer_nobody_is_waiting_for_stops_marking_its_worker() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Sculpt Studio",
                ProviderKind::ClaudeCode,
                "/workspace/ss",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let decision = store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: None,
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Which store listing?",
                summary: "Two listings could own this build.",
                reason: "Both are configured.",
                risk: "",
                evidence: "",
                suggested_action: "Use the production listing",
                allowed_actions: &["Use the production listing".to_owned()],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        // The delivery carries the answer, so it exists once the decision is
        // resolved rather than when it is asked.
        store
            .resolve_decision_request(
                decision.id,
                "Use the production listing",
                "",
                "inbox_action",
            )
            .unwrap();
        let claimed = store.claim_decision_deliveries(10).unwrap();
        assert_eq!(claimed.len(), 1, "the answer is delivered to its asker");
        let marked = store
            .fail_decision_delivery(decision.id, 11, DecisionDeliveryFailure::Uncertain)
            .unwrap();
        assert!(marked, "the delivery is recorded as unconfirmed");

        // While that terminal is still live the mark is worth carrying: the
        // operator can go and look at it.
        assert_eq!(store.forget_moot_unconfirmed_answers().unwrap(), 0);

        // Once the session has ended there is no terminal left to check and
        // nothing left to deliver.
        store.release_worker_session(session).unwrap();
        assert_eq!(store.forget_moot_unconfirmed_answers().unwrap(), 1);
        assert_eq!(store.forget_moot_unconfirmed_answers().unwrap(), 0);
    }
}
