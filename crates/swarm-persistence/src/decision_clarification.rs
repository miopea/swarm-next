//! Non-authorizing question history. Delivery remains separate from a reply.
use crate::{TaskStore, TaskStoreError};
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ClarificationDeliveryState, ControlRoomEventKind, DecisionClarificationError as Refusal,
    DecisionClarificationId, DecisionRequestId, DecisionRequestState, OperatorId, WorkerId,
    WorkerSessionId, validate_clarification_responder, validate_clarification_text,
    validate_new_clarification,
};

mod delivery;
mod reconciliation;
mod summary;
pub use summary::ClarificationAttention;
pub(crate) use summary::summaries_from;
#[cfg(test)]
mod tests;
pub use delivery::ClarificationDispatch;

pub(super) fn cancel_queued(
    tx: &Transaction<'_>,
    decision: DecisionRequestId,
) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE decision_clarifications SET delivery_state='cancelled'
         WHERE decision_id=?1 AND delivery_state='queued'",
        [decision.to_string()],
    )
}

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS decision_clarifications (
        id TEXT PRIMARY KEY CHECK(length(id)=36),
        decision_id TEXT NOT NULL REFERENCES decision_requests(id),
        round_index INTEGER NOT NULL CHECK(round_index > 0),
        operator_id TEXT NOT NULL,
        question TEXT NOT NULL CHECK(length(CAST(question AS BLOB)) BETWEEN 1 AND 4000),
        asked_at INTEGER NOT NULL CHECK(asked_at>=0),
        reply TEXT CHECK(reply IS NULL OR length(CAST(reply AS BLOB)) BETWEEN 1 AND 4000),
        replied_at INTEGER,
        replying_worker_id TEXT REFERENCES worker_profiles(id),
        replying_session_id TEXT,
        delivery_state TEXT NOT NULL DEFAULT 'queued'
          CHECK(delivery_state IN ('queued','dispatching','delivered','uncertain','cancelled')),
        claim_id TEXT,
        delivery_session_id TEXT,
        CHECK((reply IS NULL AND replied_at IS NULL AND replying_worker_id IS NULL AND replying_session_id IS NULL)
          OR (reply IS NOT NULL AND replied_at IS NOT NULL AND replying_worker_id IS NOT NULL AND replying_session_id IS NOT NULL)),
        CHECK((claim_id IS NULL AND delivery_session_id IS NULL) OR (claim_id IS NOT NULL AND delivery_session_id IS NOT NULL))
    );
    CREATE INDEX IF NOT EXISTS decision_clarifications_parent ON decision_clarifications(decision_id,asked_at,id);
    CREATE UNIQUE INDEX IF NOT EXISTS decision_clarifications_round ON decision_clarifications(decision_id,round_index);
    CREATE UNIQUE INDEX IF NOT EXISTS decision_clarifications_unanswered ON decision_clarifications(decision_id) WHERE reply IS NULL AND delivery_state!='cancelled';
    CREATE INDEX IF NOT EXISTS decision_clarifications_delivery ON decision_clarifications(delivery_state,asked_at,id);
    CREATE TABLE IF NOT EXISTS decision_clarification_notification_receipts (
        clarification_id TEXT NOT NULL REFERENCES decision_clarifications(id) ON DELETE CASCADE,
        subscription_id TEXT NOT NULL REFERENCES notification_subscriptions(device_id) ON DELETE CASCADE,
        PRIMARY KEY(clarification_id,subscription_id)
    ) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS decision_clarification_reconciliations (
        clarification_id TEXT NOT NULL REFERENCES decision_clarifications(id) ON DELETE CASCADE,
        claim_id TEXT NOT NULL,
        session_id TEXT NOT NULL,
        choice TEXT NOT NULL CHECK(choice IN ('confirm_delivered','retry')),
        operator_id TEXT NOT NULL,
        recorded_at INTEGER NOT NULL CHECK(recorded_at >= 0),
        PRIMARY KEY(clarification_id,claim_id)
    ) WITHOUT ROWID;")?;
    tx.pragma_update(
        None,
        "user_version",
        crate::DECISION_CLARIFICATION_SCHEMA_VERSION,
    )
}

/// Private exchange content is intentionally excluded from Debug and telemetry.
#[derive(Clone, Eq, PartialEq, serde::Serialize)]
pub struct DecisionClarification {
    pub id: DecisionClarificationId,
    pub decision_id: DecisionRequestId,
    pub operator_id: OperatorId,
    pub question: String,
    pub asked_at: i64,
    pub reply: Option<String>,
    pub replied_at: Option<i64>,
    pub replying_worker_id: Option<WorkerId>,
    pub replying_session_id: Option<WorkerSessionId>,
    pub delivery_state: ClarificationDeliveryState,
    pub delivery_claim_id: Option<uuid::Uuid>,
    pub delivery_session_id: Option<WorkerSessionId>,
}

const SELECT: &str = "SELECT c.id,c.decision_id,c.operator_id,c.question,c.asked_at,c.reply,c.replied_at,c.replying_worker_id,c.replying_session_id,c.delivery_state,c.claim_id,c.delivery_session_id
    FROM decision_clarifications c JOIN decision_requests d ON d.id=c.decision_id
    JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1";

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionClarification> {
    let parse = |index| row.get::<_, String>(index);
    Ok(DecisionClarification {
        id: parse(0)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        decision_id: parse(1)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        operator_id: parse(2)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        question: row.get(3)?,
        asked_at: row.get(4)?,
        reply: row.get(5)?,
        replied_at: row.get(6)?,
        replying_worker_id: row
            .get::<_, Option<String>>(7)?
            .map(|s| s.parse())
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        replying_session_id: row
            .get::<_, Option<String>>(8)?
            .map(|s| s.parse())
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        delivery_state: parse(9)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        delivery_claim_id: row
            .get::<_, Option<String>>(10)?
            .map(|id| id.parse())
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        delivery_session_id: row
            .get::<_, Option<String>>(11)?
            .map(|id| id.parse())
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn get(
    tx: &Transaction<'_>,
    id: DecisionClarificationId,
) -> Result<Option<DecisionClarification>, TaskStoreError> {
    Ok(tx
        .query_row(
            &format!("{SELECT} WHERE c.id=?1"),
            [id.to_string()],
            from_row,
        )
        .optional()?)
}

impl TaskStore {
    /// Exact-ID reads stay local to this Hive. Application authorization is required.
    /// # Errors
    /// Reports missing identity or unreadable persistence, never fabricated history.
    pub fn get_decision_clarification(
        &self,
        id: DecisionClarificationId,
    ) -> Result<DecisionClarification, TaskStoreError> {
        self.connection()?
            .query_row(
                &format!("{SELECT} WHERE c.id=?1"),
                [id.to_string()],
                from_row,
            )
            .optional()?
            .ok_or_else(|| Refusal::NotFound.into())
    }

    /// # Errors
    /// Reports an unknown parent or unavailable storage. History is capped at admission.
    pub fn decision_clarifications(
        &self,
        decision: DecisionRequestId,
    ) -> Result<Vec<DecisionClarification>, TaskStoreError> {
        self.get_decision_request(decision)?;
        let connection = self.connection()?;
        let mut query = connection.prepare(&format!(
            "{SELECT} WHERE c.decision_id=?1 ORDER BY c.round_index LIMIT 32"
        ))?;
        Ok(query
            .query_map([decision.to_string()], from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Operator-authenticated admission; this records no final answer or grant.
    /// # Errors
    /// Rejects stale parents, conflicting IDs, concurrent questions and capacity.
    pub fn ask_decision_clarification(
        &self,
        id: DecisionClarificationId,
        decision: DecisionRequestId,
        question: &str,
        now: i64,
    ) -> Result<DecisionClarification, TaskStoreError> {
        validate_clarification_text(question)?;
        if now < 0 {
            return Err(Refusal::InvalidText.into());
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        if let Some(saved) = get(&tx, id)? {
            return if saved.decision_id == decision && saved.question == question {
                Ok(saved)
            } else {
                Err(Refusal::Conflict.into())
            };
        }
        let (state, operator): (String, String) = tx
            .query_row(
                "SELECT d.state,h.operator_id FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
             JOIN hives h ON h.id=l.hive_id WHERE d.id=?1",
                [decision.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(Refusal::NotFound)?;
        // Closed history may expire; unresolved operator evidence is pinned.
        tx.execute(
            "DELETE FROM decision_clarifications WHERE decision_id IN (
            SELECT id FROM decision_requests WHERE state!='pending'
            AND COALESCE(resolved_at,withdrawn_at)>=0 AND COALESCE(resolved_at,withdrawn_at)<?1)
            AND delivery_state!='dispatching'",
            [now.saturating_sub(90 * 86400)],
        )?;
        let (count, waiting): (usize,bool) = tx.query_row(
            "SELECT COUNT(*),COALESCE(MAX(reply IS NULL AND delivery_state!='cancelled'),0) FROM decision_clarifications WHERE decision_id=?1",
            [decision.to_string()], |row| Ok((row.get(0)?,row.get(1)?)))?;
        let total: usize =
            tx.query_row("SELECT COUNT(*) FROM decision_clarifications", [], |row| {
                row.get(0)
            })?;
        validate_new_clarification(
            state.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
            question,
            waiting,
            count,
            total,
        )?;
        tx.execute("INSERT INTO decision_clarifications(id,decision_id,operator_id,question,asked_at,round_index) VALUES(?1,?2,?3,?4,?5,?6)",
            params![id.to_string(),decision.to_string(),operator,question,now,count + 1])?;
        // Asking is an explicit operator request to the author, not permission
        // for the task. Reuse the owned, bounded return queue when that author
        // is asleep. Never replace an existing promise or erase a failed attempt.
        tx.execute(
            "INSERT INTO worker_revival_intents(worker_id,recorded_at)
             SELECT w.id,?2 FROM decision_requests d JOIN worker_profiles w ON w.id=d.requesting_worker_id
             WHERE d.id=?1 AND w.archived_at IS NULL
             AND NOT EXISTS(SELECT 1 FROM worker_sessions s WHERE s.worker_id=w.id AND s.ended_at IS NULL)
             ON CONFLICT(worker_id) DO NOTHING",
            params![decision.to_string(), now],
        )?;
        crate::worker_engine_returns::check_capacity(&tx)?;
        crate::insert_control_room_event(&tx, ControlRoomEventKind::DecisionsChanged)?;
        let saved = get(&tx, id)?.ok_or(Refusal::NotFound)?;
        tx.commit()?;
        Ok(saved)
    }

    /// Exact authenticated reply. It never resolves the parent or resumes a task.
    /// # Errors
    /// Refuses other/stale workers, conflicting replies, invalid text or storage failure.
    pub fn reply_decision_clarification(
        &self,
        id: DecisionClarificationId,
        worker: WorkerId,
        session: WorkerSessionId,
        reply: &str,
        now: i64,
    ) -> Result<DecisionClarification, TaskStoreError> {
        validate_clarification_text(reply)?;
        if now < 0 {
            return Err(Refusal::InvalidText.into());
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let saved = get(&tx, id)?.ok_or(Refusal::NotFound)?;
        // Verify the actual current session even for replay; callers cannot use
        // old credentials as a new authenticated agent after replacement.
        let role: String = tx
            .query_row(
                "SELECT w.role FROM worker_profiles w
            JOIN worker_sessions s ON s.worker_id=w.id AND s.session_id=?2 AND s.ended_at IS NULL
            JOIN local_hive_identity l ON l.hive_id=w.hive_id AND l.singleton=1
            WHERE w.id=?1",
                params![worker.to_string(), session.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Refusal::Unauthorized)?;
        let (requester, state): (String, String) = tx.query_row(
            "SELECT requesting_worker_id,state FROM decision_requests WHERE id=?1",
            [saved.decision_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        validate_clarification_responder(
            requester
                .parse()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            worker,
            role == "queen",
        )?;
        if let Some(previous) = &saved.reply {
            return if previous == reply && saved.replying_worker_id == Some(worker) {
                Ok(saved)
            } else {
                Err(Refusal::Conflict.into())
            };
        }
        // A late authenticated explanation remains history. The parent is never
        // reopened, and next-move projection ignores rounds of settled requests.
        let _parent: DecisionRequestState =
            state.parse().map_err(|_| rusqlite::Error::InvalidQuery)?;
        tx.execute("UPDATE decision_clarifications SET reply=?2,replied_at=?3,replying_worker_id=?4,replying_session_id=?5,
            delivery_state=CASE WHEN delivery_state='queued' THEN 'cancelled' ELSE delivery_state END WHERE id=?1",
            params![id.to_string(),reply,now,worker.to_string(),session.to_string()])?;
        crate::insert_control_room_event(&tx, ControlRoomEventKind::DecisionsChanged)?;
        let saved = get(&tx, id)?.ok_or(Refusal::NotFound)?;
        tx.commit()?;
        Ok(saved)
    }
}
