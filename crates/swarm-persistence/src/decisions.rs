use std::{collections::HashSet, str::FromStr};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, DecisionRequest, DecisionRequestId, DecisionRequestKind,
    DecisionRequestState, DecisionUrgency, TaskId, WorkerId,
};

use super::{TaskStore, TaskStoreError, insert_control_room_event};

pub const MAX_PENDING_DECISIONS: i64 = 256;
pub const MAX_DECISION_RESULTS: i64 = 200;
const MAX_TITLE_BYTES: usize = 240;
const MAX_DETAIL_BYTES: usize = 10_000;
const MAX_ACTION_BYTES: usize = 80;
const MAX_ACTIONS: usize = 6;
const MAX_RESOLUTION_NOTE_BYTES: usize = 4_000;

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
    pub deadline: Option<i64>,
}

impl TaskStore {
    /// Creates one validated, local-Hive request and emits an inbox event atomically.
    ///
    /// # Errors
    /// Returns a validation, capacity, identity, integrity, or database error.
    pub fn create_decision_request(
        &self,
        request: &NewDecisionRequest<'_>,
    ) -> Result<DecisionRequest, TaskStoreError> {
        validate_new_request(request)?;
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
                risk, evidence, suggested_action, allowed_actions, deadline
             )
             SELECT ?1, w.hive_id, w.id, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
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
            ],
        )?;
        if inserted == 0 {
            return Err(TaskStoreError::WorkerNotFound);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
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
                        created_at, updated_at, resolved_at
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
                    created_at, updated_at, resolved_at
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

    /// Resolves a pending local-Hive request using one of its declared actions.
    ///
    /// # Errors
    /// Returns an identity, state, validation, integrity, or database error.
    pub fn resolve_decision_request(
        &self,
        id: DecisionRequestId,
        action: &str,
        note: &str,
    ) -> Result<DecisionRequest, TaskStoreError> {
        if action.is_empty()
            || action.len() > MAX_ACTION_BYTES
            || note.len() > MAX_RESOLUTION_NOTE_BYTES
        {
            return Err(TaskStoreError::InvalidDecisionResolution);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, actions): (String, String) = transaction
            .query_row(
                "SELECT d.state, d.allowed_actions FROM decision_requests d
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
        let actions: Vec<String> = serde_json::from_str(&actions)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        if !actions.iter().any(|candidate| candidate == action) {
            return Err(TaskStoreError::InvalidDecisionResolution);
        }
        let updated = transaction.execute(
            "UPDATE decision_requests SET
                 state = 'resolved', resolution_action = ?2, resolution_note = ?3,
                 resolved_by_operator_id = (
                     SELECT h.operator_id FROM local_hive_identity l
                     JOIN hives h ON h.id = l.hive_id WHERE l.singleton = 1
                 ),
                 resolved_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?1 AND state = 'pending'",
            params![id.to_string(), action, note],
        )?;
        if updated != 1 {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_decision_request(id)
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
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
        resolved_at: row.get(19)?,
    })
}

fn parse_id<T: FromStr>(value: &str) -> rusqlite::Result<T> {
    T::from_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskPriority};

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
            store.resolve_decision_request(created.id, "other", ""),
            Err(TaskStoreError::InvalidDecisionResolution)
        ));

        let resolved = store
            .resolve_decision_request(created.id, "wait", "Operator is still testing.")
            .unwrap();
        assert_eq!(resolved.state, DecisionRequestState::Resolved);
        assert_eq!(resolved.resolution_action.as_deref(), Some("wait"));
        assert!(resolved.resolved_by_operator_id.is_some());
        assert!(matches!(
            store.resolve_decision_request(created.id, "wait", "again"),
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
}
