use std::{
    collections::{BTreeMap, HashSet},
    str::FromStr,
};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, DecisionDeliveryState, DecisionQuestion, DecisionRequest,
    DecisionRequestId, DecisionRequestKind, DecisionRequestState, DecisionUrgency,
    MAX_DECISION_QUESTION_HEADER_BYTES, MAX_DECISION_QUESTION_OPTION_BYTES,
    MAX_DECISION_QUESTION_OPTIONS, MAX_DECISION_QUESTION_TEXT_BYTES, MAX_DECISION_QUESTIONS,
    MIN_DECISION_QUESTION_OPTIONS, TaskId, WorkerId, WorkerSessionId,
};

use super::{
    DECISION_QUESTIONS_SCHEMA_VERSION, DECISION_RESOLUTION_SURFACE_SCHEMA_VERSION, TaskStore,
    TaskStoreError, insert_control_room_event,
};

/// Carries the questions an interview asks and the answers it collects.
///
/// Both default to empty, which is exactly what a ruling holds, so every record
/// written before interviews existed keeps behaving as it did.
///
/// # Errors
/// Returns an error when the step cannot be applied.
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionDispatch {
    pub decision_id: DecisionRequestId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub action: String,
    pub note: String,
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
    pub reason: &'a str,
    pub risk: &'a str,
    pub evidence: &'a str,
    pub suggested_action: &'a str,
    pub allowed_actions: &'a [String],
    /// Present makes this an interview rather than a ruling. Empty is a ruling.
    pub questions: &'a [DecisionQuestion],
    pub deadline: Option<i64>,
}

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
        let actions = serde_json::to_string(request.allowed_actions)
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
                risk, evidence, suggested_action, allowed_actions, deadline, questions
             )
             SELECT ?1, w.hive_id, w.id, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
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
                        resolution_surface, questions, resolution_answers,
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
                    resolution_surface, questions, resolution_answers,
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
            // A ruling is resolved by choosing one of its actions.
            return Err(TaskStoreError::InvalidDecisionResolution);
        }
        let answered_every_question = declared.iter().all(|question| {
            answers
                .get(&question.header)
                .is_some_and(|given| given.iter().any(|value| !value.trim().is_empty()))
        });
        if !answered_every_question || answers.len() != declared.len() {
            return Err(TaskStoreError::IncompleteDecisionAnswers);
        }
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
                        d.resolution_action, d.resolution_note
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

fn validate_new_request(request: &NewDecisionRequest<'_>) -> Result<(), TaskStoreError> {
    let content = [
        request.title,
        request.reason,
        request.risk,
        request.evidence,
        request.suggested_action,
    ];
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
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
        resolved_at: row.get(22)?,
        delivery_state: row
            .get::<_, Option<String>>(23)?
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

    fn request(worker_id: WorkerId, actions: &[String]) -> NewDecisionRequest<'_> {
        NewDecisionRequest {
            requesting_worker_id: worker_id,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency: DecisionUrgency::Normal,
            title: "Choose the rollout window",
            reason: "The change is ready but needs operator timing.",
            risk: "An active user could see a brief reconnect.",
            evidence: "All automated checks passed.",
            suggested_action: "Deploy after the current session.",
            allowed_actions: actions,
            questions: &[],
            deadline: None,
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
}
