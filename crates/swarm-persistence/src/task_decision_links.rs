use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use swarm_domain::{
    DecisionRequestId, DecisionRequestState, TaskActivityActor, TaskDecisionLink,
    TaskDecisionLinkError, TaskId, TaskState, validate_decision_link_reason,
    validate_task_decision_link,
};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

#[cfg(test)]
mod tests;

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_decision_links (
            task_id TEXT NOT NULL REFERENCES tasks(id),
            decision_id TEXT NOT NULL REFERENCES decision_requests(id),
            reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB)) BETWEEN 1 AND 2048),
            actor_kind TEXT NOT NULL CHECK(actor_kind IN ('operator','worker')),
            actor_id TEXT,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(task_id, decision_id)
        );
        CREATE INDEX IF NOT EXISTS task_decision_links_by_decision
            ON task_decision_links(decision_id, task_id);
        CREATE INDEX IF NOT EXISTS decision_requests_by_task_identity
            ON decision_requests(task_id, id);
        CREATE VIEW IF NOT EXISTS task_decision_membership AS
            SELECT task_id, id AS decision_id FROM decision_requests WHERE task_id IS NOT NULL
            UNION SELECT task_id, decision_id FROM task_decision_links;",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::TASK_DECISION_LINKS_SCHEMA_VERSION,
    )
}

fn local_states(
    tx: &Transaction<'_>,
    task: TaskId,
    decision: DecisionRequestId,
) -> Result<(TaskState, DecisionRequestState, Option<String>), TaskStoreError> {
    let row: (String, String, Option<String>) = tx
        .query_row(
            "SELECT t.state,d.state,d.task_id FROM tasks t JOIN decision_requests d ON d.id=?2
         WHERE t.id=?1 AND t.removed_at IS NULL AND t.hive_id=d.hive_id
           AND t.hive_id=(SELECT hive_id FROM local_hive_identity WHERE singleton=1)",
            params![task.to_string(), decision.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(TaskStoreError::NotFound)?;
    Ok((
        row.0.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
        row.1.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
        row.2,
    ))
}

fn authorize(tx: &Transaction<'_>, actor: &TaskActivityActor) -> Result<(), TaskStoreError> {
    crate::task_prerequisites::authorize(tx, actor).map_err(|error| match error {
        TaskStoreError::TaskPrerequisite(swarm_domain::TaskPrerequisiteError::Unauthorized) => {
            TaskDecisionLinkError::Unauthorized.into()
        }
        other => other,
    })
}

fn audit(
    tx: &Transaction<'_>,
    task: TaskId,
    decision: DecisionRequestId,
    reason: &str,
    actor: &TaskActivityActor,
    now: i64,
    operation: &str,
) -> Result<(), TaskStoreError> {
    tx.execute(
        "INSERT INTO task_activity(task_id,kind,note,actor_kind,actor_id,occurred_at)
         VALUES (?1,'noted',?2,?3,?4,?5)",
        params![task.to_string(), format!("Decision blocker {operation}: {decision}. {reason}. This link does not extend command permission."),
            actor.kind.to_string(), actor.id, now],
    )?;
    tx.execute(
        "UPDATE tasks SET updated_at=?2 WHERE id=?1",
        params![task.to_string(), now],
    )?;
    crate::insert_control_room_event(tx, swarm_domain::ControlRoomEventKind::TasksChanged)?;
    Ok(())
}

impl TaskStore {
    /// Add an explicit shared blocker; never changes the original approval scope.
    ///
    /// # Errors
    /// Refuses unauthorized, foreign, settled, conflicting or over-capacity links.
    pub fn add_task_decision_link(
        &self,
        task: TaskId,
        decision: DecisionRequestId,
        reason: &str,
        expected_revision: &str,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        validate_decision_link_reason(reason)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        authorize(&tx, actor)?;
        let (task_state, decision_state, primary) = local_states(&tx, task, decision)?;
        let saved: Option<String> = tx
            .query_row(
                "SELECT reason FROM task_decision_links WHERE task_id=?1 AND decision_id=?2",
                params![task.to_string(), decision.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(saved) = saved {
            return if saved == reason {
                Ok(())
            } else {
                Err(TaskDecisionLinkError::Conflict.into())
            };
        }
        // Primary membership already exists and cannot be duplicated or removed
        // through the additional-link API.
        if primary.as_deref() == Some(task.to_string().as_str()) {
            return Ok(());
        }
        let counts: (usize, usize, usize) = tx.query_row(
            "SELECT (SELECT count(*) FROM task_decision_links WHERE decision_id=?1),
                    (SELECT count(*) FROM task_decision_links WHERE task_id=?2),
                    (SELECT count(*) FROM task_decision_links)",
            params![decision.to_string(), task.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        validate_task_decision_link(
            task_state,
            decision_state,
            reason,
            counts.0,
            counts.1,
            counts.2,
        )?;
        require_revision(&tx, task, expected_revision)?;
        tx.execute(
            "INSERT INTO task_decision_links(task_id,decision_id,reason,actor_kind,actor_id,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![task.to_string(), decision.to_string(), reason, actor.kind.to_string(), actor.id, now],
        )?;
        audit(&tx, task, decision, reason, actor, now, "added")?;
        tx.commit()?;
        Ok(())
    }

    /// Remove only an additional blocker, retaining decision and task history.
    ///
    /// # Errors
    /// Refuses unauthorized, foreign/missing identities and invalid explanations.
    pub fn remove_task_decision_link(
        &self,
        task: TaskId,
        decision: DecisionRequestId,
        reason: &str,
        expected_revision: &str,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        validate_decision_link_reason(reason)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        authorize(&tx, actor)?;
        local_states(&tx, task, decision)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_decision_links WHERE task_id=?1 AND decision_id=?2)",
            params![task.to_string(), decision.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(false);
        }
        require_revision(&tx, task, expected_revision)?;
        let removed = tx.execute(
            "DELETE FROM task_decision_links WHERE task_id=?1 AND decision_id=?2",
            params![task.to_string(), decision.to_string()],
        )? > 0;
        if removed {
            audit(&tx, task, decision, reason, actor, now, "removed")?;
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Read additional explicit relations; does not infer links or grant permission.
    ///
    /// # Errors
    /// Refuses malformed or over-capacity stored relations.
    pub fn task_decision_links(
        &self,
        task: TaskId,
    ) -> Result<Vec<TaskDecisionLink>, TaskStoreError> {
        let connection = self.connection()?;
        let mut query = connection.prepare(
            "SELECT decision_id,reason,created_at FROM task_decision_links WHERE task_id=?1
             ORDER BY decision_id LIMIT 33",
        )?;
        let links = query
            .query_map([task.to_string()], |row| {
                Ok(TaskDecisionLink {
                    task_id: task,
                    decision_id: parse_domain_id(&row.get::<_, String>(0)?)?,
                    reason: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if links.len() > swarm_domain::MAX_TASK_DECISION_LINKS {
            return Err(TaskDecisionLinkError::Capacity.into());
        }
        Ok(links)
    }
}

fn require_revision(
    tx: &Transaction<'_>,
    task: TaskId,
    expected: &str,
) -> Result<(), TaskStoreError> {
    if crate::queen_review::task_review_evidence(tx, task)?.evidence_revision != expected {
        return Err(TaskDecisionLinkError::StaleEvidence.into());
    }
    Ok(())
}
