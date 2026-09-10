//! Durable ownership only; the existing coordinator owns terminal transport.
use super::{DecisionClarification, get};
use crate::{TaskStore, TaskStoreError};
use rusqlite::params;
use swarm_domain::{
    ClarificationDeliveryOutcome, ControlRoomEventKind, DecisionClarificationError as Refusal,
    WorkerId, WorkerSessionId, clarification_delivery_result,
};

/// Exact claim/session fencing; private question text is intentionally not Debug.
#[derive(Clone)]
pub struct ClarificationDispatch {
    pub clarification: DecisionClarification,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub claim_id: uuid::Uuid,
}

impl TaskStore {
    /// Claims a bounded local batch. Provider readiness is checked by the transport.
    /// # Errors
    /// Returns persistence or identity errors.
    pub fn claim_clarification_deliveries(
        &self,
        now: i64,
    ) -> Result<Vec<ClarificationDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let candidates = {
            let mut stmt = tx.prepare(
                "SELECT c.id,d.requesting_worker_id,s.session_id
                 FROM decision_clarifications c JOIN decision_requests d ON d.id=c.decision_id
                 JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
                 JOIN worker_sessions s ON s.worker_id=d.requesting_worker_id AND s.ended_at IS NULL
                 WHERE c.delivery_state='queued' AND c.reply IS NULL AND d.state='pending'
                 AND NOT EXISTS(SELECT 1 FROM worker_engagements e
                   WHERE e.worker_id=d.requesting_worker_id AND e.expires_at>?1)
                 ORDER BY c.asked_at,c.id LIMIT 16",
            )?;
            stmt.query_map([now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
        };
        let mut claims = Vec::with_capacity(candidates.len());
        for (id, worker, session) in candidates {
            let claim_id = uuid::Uuid::now_v7();
            let changed = tx.execute(
                "UPDATE decision_clarifications SET delivery_state='dispatching',claim_id=?2,delivery_session_id=?3
                 WHERE id=?1 AND delivery_state='queued' AND reply IS NULL",
                params![id, claim_id.to_string(), session],
            )?;
            if changed != 1 {
                return Err(Refusal::Conflict.into());
            }
            let id = id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?;
            claims.push(ClarificationDispatch {
                clarification: get(&tx, id)?.ok_or(Refusal::NotFound)?,
                worker_id: worker.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                session_id: session.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                claim_id,
            });
        }
        if !claims.is_empty() {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::DecisionsChanged)?;
        }
        tx.commit()?;
        Ok(claims)
    }

    /// Rechecks exact ownership and applicability immediately before guarded transport.
    /// This does not replace the terminal host's atomic input/readiness guards.
    /// # Errors
    /// Returns persistence errors.
    pub fn clarification_dispatch_is_current(
        &self,
        claim: &ClarificationDispatch,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM decision_clarifications c
             JOIN decision_requests d ON d.id=c.decision_id
             JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
             JOIN worker_sessions s ON s.worker_id=d.requesting_worker_id AND s.session_id=c.delivery_session_id AND s.ended_at IS NULL
             WHERE c.id=?1 AND c.claim_id=?2 AND c.delivery_session_id=?3 AND d.requesting_worker_id=?4
             AND c.delivery_state='dispatching' AND c.reply IS NULL AND d.state='pending'
             AND NOT EXISTS(SELECT 1 FROM worker_engagements e WHERE e.worker_id=d.requesting_worker_id AND e.expires_at>?5))",
            params![claim.clarification.id.to_string(),claim.claim_id.to_string(),claim.session_id.to_string(),claim.worker_id.to_string(),now],
            |row| row.get(0),
        )?)
    }

    /// Acknowledges only the named claim. Ambiguous writes must never be deferred.
    /// # Errors
    /// Returns persistence errors.
    pub fn finish_clarification_delivery(
        &self,
        claim: &ClarificationDispatch,
        outcome: ClarificationDeliveryOutcome,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let applicable: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM decision_clarifications c
             JOIN decision_requests d ON d.id=c.decision_id
             WHERE c.id=?1 AND c.reply IS NULL AND d.state='pending')",
            [claim.clarification.id.to_string()],
            |row| row.get(0),
        )?;
        let state = clarification_delivery_result(outcome, applicable);
        let changed = tx.execute(
            "UPDATE decision_clarifications SET delivery_state=?4
             WHERE id=?1 AND claim_id=?2 AND delivery_session_id=?3 AND delivery_state='dispatching'
             AND EXISTS(SELECT 1 FROM decision_requests d JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
               WHERE d.id=decision_clarifications.decision_id AND d.requesting_worker_id=?5)",
            params![claim.clarification.id.to_string(),claim.claim_id.to_string(),claim.session_id.to_string(),state.to_string(),claim.worker_id.to_string()],
        )? == 1;
        if changed {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::DecisionsChanged)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Called only after acquiring exclusive coordinator ownership on startup.
    /// Interrupted submissions retain their claim evidence and never auto-retry.
    /// # Errors
    /// Returns persistence errors.
    pub fn recover_inflight_clarification_deliveries(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE decision_clarifications SET delivery_state='uncertain' WHERE delivery_state='dispatching'
             AND EXISTS(SELECT 1 FROM decision_requests d JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
               WHERE d.id=decision_clarifications.decision_id)", [],
        )?;
        if changed > 0 {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::DecisionsChanged)?;
        }
        tx.commit()?;
        Ok(changed)
    }
}
