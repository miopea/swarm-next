//! Explicit operator reconciliation; never called by an automatic retry loop.
use super::{DecisionClarification, get};
use crate::{TaskStore, TaskStoreError};
use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ClarificationReconciliation, ClarificationReconciliationChoice as Choice, ControlRoomEventKind,
    DecisionClarificationError as Refusal, validate_clarification_reconciliation,
};

impl TaskStore {
    /// The application must authenticate the operator before invoking this command.
    /// # Errors
    /// Refuses stale claims, conflicting replays, settled decisions and capacity.
    pub fn reconcile_operator_clarification(
        &self,
        request: &ClarificationReconciliation,
        now: i64,
    ) -> Result<DecisionClarification, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let saved = get(&tx, request.clarification_id)?.ok_or(Refusal::NotFound)?;
        if saved.decision_id != request.decision_id {
            return Err(Refusal::Conflict.into());
        }
        let choice = match request.choice {
            Choice::ConfirmDelivered => "confirm_delivered",
            Choice::Retry if request.acknowledged_duplicate_risk => "retry",
            Choice::Retry => return Err(Refusal::Conflict.into()),
        };
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT session_id,choice FROM decision_clarification_reconciliations
             WHERE clarification_id=?1 AND claim_id=?2",
                params![saved.id.to_string(), request.claim_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((session, previous_choice)) = previous {
            return if session == request.session_id.to_string() && previous_choice == choice {
                Ok(saved)
            } else {
                Err(Refusal::Conflict.into())
            };
        }
        let (parent, operator, count): (String, String, usize) = tx.query_row(
            "SELECT d.state,h.operator_id,(SELECT count(*) FROM decision_clarification_reconciliations r WHERE r.clarification_id=?1)
             FROM decision_requests d JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
             JOIN hives h ON h.id=l.hive_id WHERE d.id=?2",
            params![saved.id.to_string(), saved.decision_id.to_string()],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        )?;
        let target = validate_clarification_reconciliation(
            parent.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
            saved.delivery_state,
            saved.reply.is_some(),
            count,
            request,
        )?;
        let changed = tx.execute(
            "UPDATE decision_clarifications SET delivery_state=?4
             WHERE id=?1 AND claim_id=?2 AND delivery_session_id=?3 AND delivery_state='uncertain' AND reply IS NULL",
            params![saved.id.to_string(), request.claim_id.to_string(), request.session_id.to_string(), target.to_string()],
        )?;
        if changed != 1 {
            return Err(Refusal::Conflict.into());
        }
        tx.execute(
            "INSERT INTO decision_clarification_reconciliations(clarification_id,claim_id,session_id,choice,operator_id,recorded_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![saved.id.to_string(), request.claim_id.to_string(), request.session_id.to_string(), choice, operator, now],
        )?;
        crate::insert_control_room_event(&tx, ControlRoomEventKind::DecisionsChanged)?;
        let result = get(&tx, saved.id)?.ok_or(Refusal::NotFound)?;
        tx.commit()?;
        Ok(result)
    }
}
