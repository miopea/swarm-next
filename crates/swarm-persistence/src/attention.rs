use rusqlite::{Transaction, params};

use crate::TaskStoreError;

/// How long a coordinator refusal must stand before it is the operator's
/// problem, matching what the control room already shows.
pub(crate) const HELD_DELIVERY_GRACE_SECONDS: i64 = 15 * 60;
/// A refusal nothing has observed for this long is stale rather than standing.
const STALE_REFUSAL_SECONDS: i64 = 60 * 60;

/// One thing on the Needs-you queue, as the SERVER sees it.
///
/// The count the operator reads has only ever been summed in the browser
/// (`App.tsx`, `attentionCount`), which is why push could never cover it: the
/// server's enqueue was a single SELECT over `decision_requests` and had no
/// idea the other four sources existed. Widening the notification schema alone
/// would not have helped — there was nothing on this side to widen it FOR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NeedsYouSubject {
    /// Which of the five sources this came from.
    pub(crate) kind: &'static str,
    /// Stable identity for one item, and the deduplication key for a delivery.
    ///
    /// Replaces `decision_id` in that role. Two of the five sources are
    /// singletons — held deliveries and Queen automation are one card however
    /// much sits behind them — so the key cannot simply be a row id.
    pub(crate) subject_key: String,
    /// When this became the operator's problem. The watermark compares against
    /// this, so it must be when the ITEM arrived, not when it was last touched.
    pub(crate) created_at: i64,
    /// Only decisions carry one; everything else is normal.
    pub(crate) urgency: String,
}

impl NeedsYouSubject {
    /// The decision this names, for the FK that keeps a delivery cascading away
    /// when its decision is deleted.
    ///
    /// Only decisions have one. The widened CHECK requires every other kind to
    /// leave it NULL, so this must return None for them rather than inventing a
    /// value — the constraint is what stops a future source quietly reusing the
    /// decision column and making `subject_key` decorative again.
    pub(crate) fn decision_id(&self) -> Option<&str> {
        (self.kind == "decision")
            .then(|| self.subject_key.strip_prefix("decision:"))
            .flatten()
    }
}

/// Everything currently waiting on the operator, from every source.
///
/// Deliberately transaction-level SQL rather than calls to the existing
/// `&self` readers. Those take the connection mutex, and this runs inside a
/// transaction that already holds it.
///
/// Mirrors `attentionCount` in `App.tsx`. The two must agree: a badge that
/// disagrees with the page teaches the operator to stop believing the badge,
/// and a notification that disagrees with both is worse again.
pub(crate) fn needs_you_subjects(
    transaction: &Transaction<'_>,
    now: i64,
) -> Result<Vec<NeedsYouSubject>, TaskStoreError> {
    let mut subjects = Vec::new();
    subjects.extend(pending_decisions(transaction)?);
    subjects.extend(pending_assists(transaction)?);
    subjects.extend(queen_automation_attention(transaction)?);
    subjects.extend(held_deliveries(transaction, now)?);
    subjects.extend(emails_awaiting_a_reply(transaction)?);
    Ok(subjects)
}

fn pending_decisions(
    transaction: &Transaction<'_>,
) -> Result<Vec<NeedsYouSubject>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT id, created_at, urgency FROM decision_requests
         WHERE state = 'pending' AND hive_id = (
             SELECT hive_id FROM local_hive_identity WHERE singleton = 1
         )",
    )?;
    let rows = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(NeedsYouSubject {
            kind: "decision",
            subject_key: format!("decision:{id}"),
            created_at: row.get(1)?,
            urgency: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn pending_assists(transaction: &Transaction<'_>) -> Result<Vec<NeedsYouSubject>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT request_id, created_at FROM apiary_steward_assist_requests
         WHERE state = 'pending'",
    )?;
    let rows = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(NeedsYouSubject {
            kind: "assist",
            subject_key: format!("assist:{id}"),
            created_at: row.get(1)?,
            urgency: "normal".to_owned(),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Queen is stuck, or has finished and needs the operator.
///
/// Mirrors `queenAutomationNeedsAttention`, INCLUDING its suppression: a
/// completed run that needs the operator only counts when Queen also has a
/// decision pending, because otherwise the decision itself is the item and
/// counting both would show one problem as two.
fn queen_automation_attention(
    transaction: &Transaction<'_>,
) -> Result<Vec<NeedsYouSubject>, TaskStoreError> {
    let row = transaction
        .query_row(
            "SELECT state, outcome, run_id, COALESCE(finished_at, delivered_at, requested_at, updated_at)
             FROM queen_automation WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .ok();
    let Some((state, outcome, run_id, at)) = row else {
        return Ok(Vec::new());
    };
    let queen_decision_pending: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM decision_requests d
         JOIN worker_profiles w ON w.id = d.requesting_worker_id
         WHERE d.state = 'pending' AND w.role = 'queen'",
        [],
        |row| row.get(0),
    )?;
    // REPLICATED EXACTLY, INCLUDING A QUIRK. The browser computes
    //
    //   queenAutomationNeedsAttention(status, pendingQueenDecisionCount > 0)
    //     && pendingQueenDecisionCount === 0
    //
    // and that helper's second branch — completed with outcome needs_operator —
    // REQUIRES a pending Queen decision, which the `=== 0` then excludes. So
    // that branch can never contribute, and in practice this counts exactly one
    // case: an uncertain run with no Queen decision pending.
    //
    // Left as it is on purpose. Making the server disagree with the browser
    // about what Needs you holds would be a worse defect than the redundancy,
    // and changing what counts is a product decision rather than a wiring one.
    // Filed rather than fixed here.
    if !(state == "uncertain" && queen_decision_pending == 0) {
        return Ok(Vec::new());
    }
    let _ = outcome;
    Ok(vec![NeedsYouSubject {
        kind: "queen_automation",
        subject_key: format!(
            "queen-automation:{}",
            run_id.unwrap_or_else(|| "none".into())
        ),
        created_at: at,
        urgency: "normal".to_owned(),
    }])
}

/// Work a coordinator refused to hand on, standing long enough to be the
/// operator's problem. One item however many deliveries sit behind it.
fn held_deliveries(
    transaction: &Transaction<'_>,
    now: i64,
) -> Result<Vec<NeedsYouSubject>, TaskStoreError> {
    let earliest: Option<i64> = transaction.query_row(
        "SELECT MIN(first_observed_at) FROM coordinator_refusals
         WHERE cleared_at IS NULL
           AND ?1 - first_observed_at >= ?2
           AND ?1 - last_observed_at <= ?3",
        params![now, HELD_DELIVERY_GRACE_SECONDS, STALE_REFUSAL_SECONDS],
        |row| row.get(0),
    )?;
    Ok(earliest
        .map(|created_at| NeedsYouSubject {
            kind: "held_delivery",
            subject_key: "held-deliveries".to_owned(),
            created_at,
            urgency: "normal".to_owned(),
        })
        .into_iter()
        .collect())
}

/// Finished work whose requester was never answered.
///
/// The one source where the cost of silence lands on someone who is not the
/// operator: a reply sat unanswered for eleven days before anything noticed.
fn emails_awaiting_a_reply(
    transaction: &Transaction<'_>,
) -> Result<Vec<NeedsYouSubject>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT t.id, MIN(link.received_at)
         FROM tasks t
         JOIN email_message_links link ON link.task_id = t.id
         WHERE t.state = 'completed' AND t.removed_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM email_reply_deliveries reply
               WHERE reply.task_id = t.id AND reply.state = 'delivered'
           )
         GROUP BY t.id",
    )?;
    let rows = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(NeedsYouSubject {
            kind: "email_reply",
            subject_key: format!("email-reply:{id}"),
            created_at: row.get(1)?,
            urgency: "normal".to_owned(),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
