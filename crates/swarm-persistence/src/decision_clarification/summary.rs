//! One bounded projection for inbox consumers, without copying private text.
use crate::{TaskStore, TaskStoreError};
use std::collections::HashMap;
use swarm_domain::{DecisionClarificationSummary, DecisionRequestId, clarification_next_move};

/// Bounded, content-free attention independent of task assignment or membership.
#[derive(serde::Serialize)]
pub struct ClarificationAttention {
    pub total: usize,
    pub requests: Vec<swarm_domain::TaskClarificationWait>,
    pub truncated: bool,
}

impl TaskStore {
    /// Read unanswered questions, including decisions with no linked task.
    /// # Errors
    /// Unavailable or malformed evidence is an error, never an empty queue.
    pub fn clarification_attention(&self) -> Result<ClarificationAttention, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.id,c.id,d.requesting_worker_id,w.role='queen',c.delivery_state,count(*) OVER ()
             FROM decision_clarifications c JOIN decision_requests d ON d.id=c.decision_id
             JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
             JOIN worker_profiles w ON w.id=d.requesting_worker_id
             WHERE d.state='pending' AND c.reply IS NULL AND c.delivery_state!='cancelled'
             ORDER BY c.asked_at,c.id LIMIT 64",
        )?;
        let rows = statement.query_map([], |row| {
            let parse = |index| row.get::<_, String>(index);
            Ok((
                swarm_domain::TaskClarificationWait {
                    decision_id: parse(0)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    clarification_id: parse(1)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    requesting_worker_id: parse(2)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    requester_is_queen: row.get(3)?,
                    delivery_state: parse(4)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                },
                row.get::<_, usize>(5)?,
            ))
        })?;
        let mut attention = ClarificationAttention {
            total: 0,
            requests: Vec::new(),
            truncated: false,
        };
        for row in rows {
            let (request, total) = row?;
            attention.total = total;
            attention.requests.push(request);
        }
        attention.truncated = attention.total > attention.requests.len();
        Ok(attention)
    }

    /// Operator inbox and clarification facts share one database snapshot.
    /// # Errors
    /// Reports unreadable decision or clarification evidence.
    pub fn decision_inbox(&self) -> Result<Vec<swarm_domain::DecisionInboxEntry>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let decisions = {
            let mut statement = transaction.prepare(&crate::decisions::decision_inbox_sql())?;
            statement
                .query_map(
                    rusqlite::params![
                        crate::decisions::MAX_DECISION_RESULTS,
                        Option::<String>::None
                    ],
                    crate::decisions::decision_from_row,
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut summaries = summaries_from(&transaction)?;
        let inbox = decisions
            .into_iter()
            .map(|decision| swarm_domain::DecisionInboxEntry {
                clarification: summaries.remove(&decision.id),
                decision,
                // The store cannot know: the label depends on the native answer
                // resolution switch, which the application layer reads. False
                // here means "not yet decided", and false is also the safe
                // answer if nobody ever decides — it sends the operator to the
                // control room, which always works.
                terminal_answerable: false,
            })
            .collect();
        transaction.commit()?;
        Ok(inbox)
    }

    /// Reads compact facts for local decisions that have clarification history.
    /// The retained exchange table is bounded to 4096 rows; this does not perform
    /// an extra query per decision or load the question/reply bodies.
    /// # Errors
    /// Returns persistence failures rather than treating unreadable facts as clear.
    pub fn decision_clarification_summaries(
        &self,
    ) -> Result<HashMap<DecisionRequestId, DecisionClarificationSummary>, TaskStoreError> {
        let connection = self.connection()?;
        summaries_from(&connection)
    }
}

pub(crate) fn summaries_from(
    connection: &rusqlite::Connection,
) -> Result<HashMap<DecisionRequestId, DecisionClarificationSummary>, TaskStoreError> {
    let mut statement = connection.prepare(
            "SELECT c.decision_id, d.state, count(*),
             max(CASE WHEN c.reply IS NULL AND c.delivery_state!='cancelled' THEN c.id END),
             max(CASE WHEN c.reply IS NULL AND c.delivery_state!='cancelled' THEN c.delivery_state END),
             max(c.replied_at),
             (SELECT r.id FROM decision_clarifications r
              WHERE r.decision_id=c.decision_id AND r.reply IS NOT NULL
              ORDER BY r.round_index DESC LIMIT 1)
             FROM decision_clarifications c
             JOIN decision_requests d ON d.id=c.decision_id
             JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1
             GROUP BY c.decision_id, d.state",
        )?;
    let rows = statement.query_map([], |row| {
        let decision = row
            .get::<_, String>(0)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let parent = row
            .get::<_, String>(1)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let waiting = row
            .get::<_, Option<String>>(3)?
            .map(|id| id.parse())
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let delivery = row
            .get::<_, Option<String>>(4)?
            .map(|state| state.parse())
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok((
            decision,
            DecisionClarificationSummary {
                round_count: row.get(2)?,
                waiting_clarification_id: waiting,
                delivery_state: delivery,
                latest_reply_at: row.get(5)?,
                latest_reply_id: row
                    .get::<_, Option<String>>(6)?
                    .map(|id| id.parse())
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                next_move: clarification_next_move(parent, waiting.is_some()),
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
