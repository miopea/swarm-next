//! Recovery identity comes from the live attention predicate and one task snapshot.
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    QueenRecoveryAssessment, QueenRecoveryFacts, QueenRecoveryIdentity, QueenRecoveryRecord,
    TaskActivityActor, TaskId, WorkerId, WorkerSessionId, assess_queen_recovery,
};

use crate::{TaskStore, TaskStoreError};

pub(super) fn migrate(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_recovery_receipts (
        task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
        run_id TEXT NOT NULL, attention_id TEXT NOT NULL,
        worker_id TEXT NOT NULL REFERENCES worker_profiles(id), session_id TEXT NOT NULL,
        accepted_revision TEXT NOT NULL CHECK(length(accepted_revision)=64),
        terminal_revision TEXT CHECK(terminal_revision IS NULL OR length(terminal_revision)=64),
        input_payload TEXT NOT NULL CHECK(length(input_payload)<=8192),
        recorded_at INTEGER NOT NULL
    );",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::QUEEN_RECOVERY_RECEIPTS_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Read the same live recovery obligations Queen sees, with exact transport
    /// evidence, in one database snapshot. No terminal reads or writes occur.
    ///
    /// # Errors
    /// Propagates corrupt stored evidence instead of presenting an empty queue.
    pub fn recovery_queue_snapshot(
        &self,
        observations: &[swarm_domain::RecoveryQueueObservation],
        now: i64,
    ) -> Result<swarm_domain::RecoveryQueueSnapshot, TaskStoreError> {
        use swarm_domain::{RecoveryQueueDelivery, RecoveryQueueItem, RecoveryQueueSnapshot};
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT action.id, task.id, worker.id, session.session_id,
                    task.updated_at, action.finished_at, action.reason,
                    EXISTS(SELECT 1 FROM worker_engagements e WHERE e.worker_id=worker.id AND e.expires_at>?1),
                    (SELECT json_object('message_id',m.id,'state',d.state,'updated_at',d.updated_at)
                     FROM task_messages m JOIN task_message_deliveries d ON d.message_id=m.id
                     WHERE m.task_id=task.id AND m.sender='queen' AND m.recipient_worker_id=worker.id
                       AND m.created_at>=action.finished_at AND d.superseded=0
                       AND d.state IN ('queued','dispatching','delivered','uncertain','rejected')
                       AND (d.state='queued' OR (d.state='delivered' AND m.delivered_session_id=session.session_id)
                            OR (d.state IN ('dispatching','uncertain','rejected') AND d.session_id=session.session_id))
                     ORDER BY m.created_at DESC,m.id DESC LIMIT 1),
                    (SELECT r.input_payload FROM queen_recovery_receipts r
                     WHERE r.task_id=task.id AND r.attention_id=action.id
                       AND r.worker_id=worker.id AND r.session_id=session.session_id)
             {} AND action.kind IN ('stale_owned_work_attention',
                 'assigned_ready_work_not_started_attention','owned_work_never_briefed_attention')
             ORDER BY action.finished_at DESC,action.id DESC LIMIT 33",
            crate::coordinator::LIVE_ATTENTION_SOURCE
        ))?;
        let rows = statement
            .query_map([now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut snapshot = RecoveryQueueSnapshot {
            items: Vec::new(),
            truncated: rows.len() > 32,
        };
        for (
            attention_id,
            task_id,
            worker_id,
            session_id,
            task_revision,
            observed_at,
            reason,
            engaged,
            delivery,
            assessment,
        ) in rows.into_iter().take(32)
        {
            let session_id = session_id
                .parse()
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let delivery: Option<RecoveryQueueDelivery> = delivery
                .map(|raw| serde_json::from_str(&raw))
                .transpose()
                .map_err(|_| {
                    TaskStoreError::IntegrityFailure("invalid recovery queue delivery".into())
                })?;
            let observation = observations
                .iter()
                .find(|item| item.session_id == session_id)
                .copied();
            let Some(state) = swarm_domain::recovery_queue_state(
                session_id,
                observation,
                engaged,
                delivery.as_ref().map(|item| item.state.as_str()),
            ) else {
                continue;
            };
            snapshot.items.push(RecoveryQueueItem {
                attention_id,
                task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                worker_id: worker_id
                    .parse()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                session_id,
                task_revision,
                observed_at,
                reason,
                state,
                delivery,
                last_assessment: assessment
                    .map(|raw| read_saved_recovery(&raw))
                    .transpose()?,
            });
        }
        Ok(snapshot)
    }

    /// Evaluate all current recovery obligations against freshly obtained facts.
    /// A partial observation pass cannot certify complete recovery coverage.
    ///
    /// # Errors
    /// Propagates source/receipt corruption rather than declaring the fleet clear.
    pub fn queen_recovery_coverage(
        &self,
        run_id: &str,
        observations: &[QueenRecoveryFacts],
        complete: bool,
        now: i64,
    ) -> Result<swarm_domain::QueenReviewCoverage, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let result = recovery_coverage(&tx, run_id, observations, complete, now)?;
        tx.commit()?;
        Ok(result)
    }

    /// Obtain a current recovery identity without certifying terminal health.
    ///
    /// # Errors
    /// Refuses source overflow, invalid stored identities and unavailable storage.
    pub fn queen_recovery_identity(
        &self,
        attention_id: &str,
    ) -> Result<Option<QueenRecoveryIdentity>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let identity = recovery_identity(&transaction, attention_id)?;
        transaction.commit()?;
        Ok(identity)
    }

    /// Save a bounded, fenced assessment, not a task transition or authorization.
    /// Terminal facts must be freshly obtained by the application, never parsed
    /// from the agent's request. Durable facts are re-derived inside this write.
    ///
    /// # Errors
    /// Refuses stale identities, invalid sources, uncovered claims and capacity.
    pub fn record_queen_recovery(
        &self,
        input: &QueenRecoveryRecord,
        observed: &QueenRecoveryFacts,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<QueenRecoveryIdentity, TaskStoreError> {
        input.validate().map_err(refused)?;
        let payload = serde_json::to_string(input)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        if payload.len() > 8192 {
            return Err(refused("recovery assessment exceeds its payload bound"));
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        crate::task_prerequisites::authorize(&tx, actor)?;
        let mut current = recovery_identity(&tx, &input.identity.attention_id)?
            .ok_or_else(|| refused("recovery obligation changed; read current attention"))?;
        current
            .terminal_revision
            .clone_from(&observed.identity.terminal_revision);
        let mut facts = observed.clone();
        refresh_durable_facts(&tx, &mut facts, now)?;
        // The authenticated command explicitly records a checked judgment for
        // this run. This is not a boolean supplied by an observation or proof
        // that work completed. Validation requires condition, evidence, source.
        facts.verified_external_wait = matches!(
            input.disposition,
            swarm_domain::QueenRecoveryDisposition::VerifiedExternalWait { .. }
        );
        let replay: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_recovery_receipts WHERE task_id=?1
             AND input_payload=?2 AND accepted_revision=?3 AND terminal_revision IS ?4)",
            params![
                current.task_id.to_string(),
                payload,
                current.evidence_revision,
                current.terminal_revision
            ],
            |row| row.get(0),
        )?;
        if replay
            && observed.identity == current
            && assess_queen_recovery(&current, &facts, &input.disposition)
                != QueenRecoveryAssessment::Uncovered
        {
            return Ok(current);
        }
        if current != input.identity || observed.identity != current {
            return Err(refused(
                "task, session or terminal evidence changed; observe again",
            ));
        }
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_automation WHERE id=1 AND run_id=?1
             AND delivery_session_id IS NOT NULL
             AND (state IN ('running','uncertain')
                 OR (state IN ('queued','delivering') AND delivered_at IS NOT NULL)))",
            [&input.run_id],
            |row| row.get(0),
        )?;
        if !active {
            return Err(refused(
                "recovery assessment requires the current unfinished delivered review",
            ));
        }
        // External conditions remain explicit authenticated judgments, not
        // machine-verified facts or new authority to resume a task.
        if assess_queen_recovery(&input.identity, &facts, &input.disposition)
            == QueenRecoveryAssessment::Uncovered
        {
            return Err(refused(input.disposition.evidence_requirement()));
        }
        tx.execute("DELETE FROM queen_recovery_receipts WHERE task_id IN
            (SELECT id FROM tasks WHERE removed_at IS NOT NULL OR state IN ('completed','abandoned'))", [])?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM queen_recovery_receipts WHERE task_id!=?1",
            [current.task_id.to_string()],
            |row| row.get(0),
        )?;
        if count >= 256 {
            return Err(refused(
                "recovery receipt capacity reached; coverage is incomplete",
            ));
        }
        tx.execute(
            "INSERT INTO task_activity (task_id,kind,note,actor_kind,actor_id,occurred_at)
            VALUES (?1,'corrected',?2,?3,?4,?5)",
            params![
                current.task_id.to_string(),
                recovery_audit_note(input),
                actor.kind.to_string(),
                actor.id,
                now
            ],
        )?;
        current.evidence_revision =
            crate::queen_review::task_review_evidence(&tx, current.task_id)?.evidence_revision;
        tx.execute("INSERT INTO queen_recovery_receipts
            (task_id,run_id,attention_id,worker_id,session_id,accepted_revision,terminal_revision,input_payload,recorded_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(task_id) DO UPDATE SET
            run_id=excluded.run_id,attention_id=excluded.attention_id,worker_id=excluded.worker_id,
            session_id=excluded.session_id,accepted_revision=excluded.accepted_revision,
            terminal_revision=excluded.terminal_revision,input_payload=excluded.input_payload,recorded_at=excluded.recorded_at",
            params![current.task_id.to_string(), input.run_id, current.attention_id, current.worker_id.to_string(),
                current.session_id.to_string(), current.evidence_revision, current.terminal_revision, payload, now])?;
        crate::insert_control_room_event(&tx, swarm_domain::ControlRoomEventKind::TasksChanged)?;
        tx.commit()?;
        Ok(current)
    }
}

fn recovery_audit_note(input: &QueenRecoveryRecord) -> String {
    let judgment = match &input.disposition {
        swarm_domain::QueenRecoveryDisposition::VerifiedExternalWait { checked_evidence } => {
            format!(
                "\nExternal-condition judgment (not completion or authorization): {checked_evidence}"
            )
        }
        _ => String::new(),
    };
    format!(
        "Queen recovery assessment: {}\nChecked source: {}{judgment}",
        input.reason, input.source
    )
}

fn refused(reason: &str) -> TaskStoreError {
    TaskStoreError::RecoveryAssessmentRefused(reason.into())
}

fn read_saved_recovery(payload: &str) -> Result<QueenRecoveryRecord, TaskStoreError> {
    let saved: QueenRecoveryRecord = serde_json::from_str(payload).map_err(|_| {
        TaskStoreError::IntegrityFailure("stored recovery receipt is unreadable".into())
    })?;
    saved
        .validate()
        .map_err(|reason| TaskStoreError::IntegrityFailure(reason.into()))?;
    Ok(saved)
}

pub(super) fn recovery_coverage(
    connection: &Connection,
    run_id: &str,
    observations: &[QueenRecoveryFacts],
    complete: bool,
    now: i64,
) -> Result<swarm_domain::QueenReviewCoverage, TaskStoreError> {
    use swarm_domain::{QueenRecoveryDisposition, QueenReviewCoverage};
    if !complete || observations.len() > 256 {
        return Ok(QueenReviewCoverage::Unavailable);
    }
    let mut observed = std::collections::HashMap::new();
    for facts in observations {
        if observed
            .insert(facts.identity.attention_id.as_str(), facts)
            .is_some()
        {
            return Ok(QueenReviewCoverage::Unavailable);
        }
    }
    let ids = {
        let mut statement = connection.prepare(&format!("SELECT action.id {} AND action.kind IN
            ('stale_owned_work_attention','assigned_ready_work_not_started_attention','owned_work_never_briefed_attention')
            ORDER BY action.id LIMIT 257", crate::coordinator::LIVE_ATTENTION_SOURCE))?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    if ids.len() > 256 {
        return Ok(QueenReviewCoverage::Unavailable);
    }
    let mut missing = Vec::new();
    let mut waiting = 0;
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        let Some(mut current) = recovery_identity(connection, &id)? else {
            continue;
        };
        if !seen.insert(current.task_id) {
            return Ok(QueenReviewCoverage::Unavailable);
        }
        let Some(facts) = observed.get(id.as_str()) else {
            missing.push(current.task_id);
            continue;
        };
        current
            .terminal_revision
            .clone_from(&facts.identity.terminal_revision);
        let mut fresh = (*facts).clone();
        refresh_durable_facts(connection, &mut fresh, now)?;
        fresh.verified_external_wait = false;
        // Current machine evidence can settle a historical stall observation;
        // do not require Queen to approve a busy worker or genuine protected input.
        let mut dispositions = vec![
            QueenRecoveryDisposition::ObservedWorking,
            QueenRecoveryDisposition::ProtectOperatorInput,
        ];
        if let Some(message_id) = &fresh.pending_message_id {
            dispositions.push(QueenRecoveryDisposition::AwaitDelivery {
                message_id: message_id.clone(),
            });
        }
        if let Some(decision_id) = fresh.pending_decision_id {
            dispositions.push(QueenRecoveryDisposition::AwaitOperator { decision_id });
        }
        let receipt: Option<String> = connection
            .query_row(
                "SELECT input_payload FROM queen_recovery_receipts
            WHERE task_id=?1 AND run_id=?2 AND attention_id=?3 AND worker_id=?4 AND session_id=?5
            AND accepted_revision=?6 AND terminal_revision IS ?7",
                params![
                    current.task_id.to_string(),
                    run_id,
                    current.attention_id,
                    current.worker_id.to_string(),
                    current.session_id.to_string(),
                    current.evidence_revision,
                    current.terminal_revision
                ],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(payload) = receipt {
            let saved = read_saved_recovery(&payload)?;
            // The query fences this judgment to this exact run and current
            // task/session/terminal identity. It never carries to the next run.
            fresh.verified_external_wait = matches!(
                saved.disposition,
                QueenRecoveryDisposition::VerifiedExternalWait { .. }
            );
            dispositions.push(saved.disposition);
        }
        let assessment = dispositions
            .iter()
            .map(|disposition| assess_queen_recovery(&current, &fresh, disposition))
            .find(|result| *result != QueenRecoveryAssessment::Uncovered);
        match assessment {
            Some(QueenRecoveryAssessment::Working) => {}
            Some(QueenRecoveryAssessment::Waiting) => waiting += 1,
            _ => missing.push(current.task_id),
        }
    }
    Ok(if missing.is_empty() {
        QueenReviewCoverage::Covered {
            waiting_obligations: waiting,
        }
    } else {
        QueenReviewCoverage::Missing { task_ids: missing }
    })
}

fn refresh_durable_facts(
    connection: &Connection,
    facts: &mut QueenRecoveryFacts,
    now: i64,
) -> Result<(), TaskStoreError> {
    facts.operator_engaged = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_engagements
        WHERE worker_id=?1 AND expires_at>?2)",
        params![facts.identity.worker_id.to_string(), now],
        |row| row.get(0),
    )?;
    facts.pending_message_id = connection
        .query_row(
            "SELECT m.id FROM task_messages m
        JOIN task_message_deliveries d ON d.message_id=m.id
        WHERE m.task_id=?1 AND m.sender='queen' AND m.recipient_worker_id=?2
          AND d.state IN ('queued','dispatching') AND d.superseded=0
          AND (d.state='queued' OR d.session_id=?3)
        ORDER BY m.created_at DESC,m.id DESC LIMIT 1",
            params![
                facts.identity.task_id.to_string(),
                facts.identity.worker_id.to_string(),
                facts.identity.session_id.to_string()
            ],
            |row| row.get(0),
        )
        .optional()?;
    let decision: Option<String> = connection
        .query_row(
            "SELECT id FROM decision_requests
        WHERE id IN (SELECT decision_id FROM task_decision_membership WHERE task_id=?1)
          AND state='pending' ORDER BY id LIMIT 1",
            [facts.identity.task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    facts.pending_decision_id = decision
        .map(|id| {
            id.parse().map_err(|_| {
                TaskStoreError::IntegrityFailure("invalid recovery decision identity".into())
            })
        })
        .transpose()?;
    Ok(())
}

fn recovery_identity(
    connection: &Connection,
    attention_id: &str,
) -> Result<Option<QueenRecoveryIdentity>, TaskStoreError> {
    let identities: Option<(String, String, String)> = connection
        .query_row(
            &format!(
                "SELECT task.id, worker.id, session.session_id {} AND action.id=?1
                 AND action.kind IN ('stale_owned_work_attention',
                     'assigned_ready_work_not_started_attention',
                     'owned_work_never_briefed_attention')",
                crate::coordinator::LIVE_ATTENTION_SOURCE,
            ),
            [attention_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((task, worker, session)) = identities else {
        return Ok(None);
    };
    let invalid = || TaskStoreError::IntegrityFailure("invalid stored recovery identity".into());
    let task_id = TaskId::from_str(&task).map_err(|_| invalid())?;
    let identity = QueenRecoveryIdentity {
        task_id,
        worker_id: WorkerId::from_str(&worker).map_err(|_| invalid())?,
        session_id: WorkerSessionId::from_str(&session).map_err(|_| invalid())?,
        attention_id: attention_id.to_owned(),
        terminal_revision: None,
        evidence_revision: crate::queen_review::task_review_evidence(connection, task_id)?
            .evidence_revision,
    };
    Ok(Some(identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        ProviderKind, QueenRecoveryDisposition, RecoveryTerminalActivity, TaskState,
    };
    const NOW: i64 = 4_000_000_000;

    #[test]
    fn recovery_queue_reports_overflow_instead_of_certifying_a_partial_fleet() {
        let store = TaskStore::in_memory().unwrap();
        for index in 0..33 {
            let workspace = format!("/workspace/bounded-{index}");
            let worker = store
                .create_worker(
                    &format!("Bounded {index}"),
                    ProviderKind::ClaudeCode,
                    &workspace,
                    false,
                    1,
                )
                .unwrap();
            store
                .bind_worker_session(worker.id, WorkerSessionId::new())
                .unwrap();
            let task = store
                .create_task("Unfinished fictional work", &workspace)
                .unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store
                .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
                .unwrap();
            store.transition_task(task.id, TaskState::Active).unwrap();
            let candidate = store
                .stale_owned_work_candidates(NOW, 600)
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.task_id == task.id)
                .unwrap();
            store
                .record_stale_owned_work_attention(
                    &candidate,
                    NOW,
                    600,
                    crate::BackgroundWorkReading::NoneVisible,
                )
                .unwrap();
        }
        let snapshot = store.recovery_queue_snapshot(&[], NOW).unwrap();
        assert_eq!(snapshot.items.len(), 32);
        assert!(snapshot.truncated);
        assert!(
            snapshot
                .items
                .iter()
                .all(|item| item.state == swarm_domain::RecoveryQueueState::ObservationUnavailable)
        );
    }

    #[test]
    fn recovery_queue_tracks_exact_delivery_without_settling_the_task() {
        use swarm_domain::{RecoveryQueueObservation, RecoveryQueueState};
        let store = TaskStore::in_memory().unwrap();
        let (input, _) = fixture(&store);
        let id = input.identity;
        let observations = [RecoveryQueueObservation {
            session_id: id.session_id,
            activity: RecoveryTerminalActivity::Resting,
            background_work: false,
        }];
        let read = |now| store.recovery_queue_snapshot(&observations, now).unwrap();
        assert_eq!(
            read(NOW).items[0].state,
            RecoveryQueueState::NeedsQueenCheck
        );
        assert_eq!(
            store.recovery_queue_snapshot(&[], NOW).unwrap().items[0].state,
            RecoveryQueueState::ObservationUnavailable
        );
        let message = store
            .send_task_message(
                id.task_id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(id.worker_id),
                "Continue this fictional task",
                NOW + 1,
            )
            .unwrap();
        let queued = read(NOW + 1);
        assert_eq!(queued.items[0].state, RecoveryQueueState::AwaitingDelivery);
        assert_eq!(
            queued.items[0].delivery.as_ref().unwrap().message_id,
            message.id
        );
        store
            .mark_task_message_delivered(&message.id, id.session_id, NOW + 2)
            .unwrap();
        assert_eq!(
            read(NOW + 2).items[0].state,
            RecoveryQueueState::VerifyWorkerResponse
        );
        assert_eq!(store.get_task(id.task_id).unwrap().state, TaskState::Active);
        store
            .renew_worker_engagement(id.session_id, None, NOW + 3, 60)
            .unwrap();
        assert!(read(NOW + 4).items.is_empty());
        assert_eq!(read(NOW + 64).items.len(), 1);
        let working = [RecoveryQueueObservation {
            activity: RecoveryTerminalActivity::Working,
            ..observations[0]
        }];
        assert!(
            store
                .recovery_queue_snapshot(&working, NOW + 64)
                .unwrap()
                .items
                .is_empty()
        );
        store
            .transition_task(id.task_id, TaskState::Review)
            .unwrap();
        assert!(read(NOW + 65).items.is_empty());
    }

    #[test]
    fn recovery_queue_does_not_borrow_old_or_other_session_messages() {
        use swarm_domain::{RecoveryQueueObservation, RecoveryQueueState};
        let store = TaskStore::in_memory().unwrap();
        let (input, _) = fixture(&store);
        let id = input.identity;
        let observations = [RecoveryQueueObservation {
            session_id: id.session_id,
            activity: RecoveryTerminalActivity::Resting,
            background_work: false,
        }];
        store
            .send_task_message(
                id.task_id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(id.worker_id),
                "Older request",
                NOW - 1,
            )
            .unwrap();
        assert!(
            store
                .recovery_queue_snapshot(&observations, NOW)
                .unwrap()
                .items[0]
                .delivery
                .is_none()
        );
        let message = store
            .send_task_message(
                id.task_id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(id.worker_id),
                "Different session",
                NOW + 1,
            )
            .unwrap();
        store
            .mark_task_message_delivered(&message.id, WorkerSessionId::new(), NOW + 2)
            .unwrap();
        let snapshot = store
            .recovery_queue_snapshot(&observations, NOW + 2)
            .unwrap();
        assert_eq!(snapshot.items[0].state, RecoveryQueueState::NeedsQueenCheck);
        assert!(snapshot.items[0].delivery.is_none());
        assert!(!snapshot.truncated);
    }

    fn fixture(store: &TaskStore) -> (QueenRecoveryRecord, QueenRecoveryFacts) {
        let worker = store
            .create_worker(
                "Recovery fixture",
                ProviderKind::ClaudeCode,
                "/workspace/demo",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store
            .create_task("Recovery fixture task", "/workspace/demo")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        let candidate = store
            .stale_owned_work_candidates(NOW, 600)
            .unwrap()
            .pop()
            .unwrap();
        store
            .record_stale_owned_work_attention(
                &candidate,
                NOW,
                600,
                crate::BackgroundWorkReading::NoneVisible,
            )
            .unwrap();
        let attention = store
            .current_coordinator_attention(NOW)
            .unwrap()
            .into_iter()
            .find(|a| a.task_id == task.id)
            .unwrap();
        let mut identity = store
            .queen_recovery_identity(&attention.action_id)
            .unwrap()
            .unwrap();
        identity.terminal_revision = Some("b".repeat(64));
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.request_queen_automation_run(NOW).unwrap();
        let run = store.claim_queen_automation(NOW).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, NOW)
            .unwrap();
        let facts = QueenRecoveryFacts {
            identity: identity.clone(),
            terminal_is_current: true,
            activity: RecoveryTerminalActivity::Working,
            operator_engaged: false,
            unsent_input: Some(false),
            pending_message_id: None,
            pending_decision_id: None,
            verified_external_wait: false,
        };
        (
            QueenRecoveryRecord {
                run_id: run.run_id,
                identity,
                disposition: QueenRecoveryDisposition::ObservedWorking,
                reason: "Current canonical terminal is working".into(),
                source: "Isolated trusted terminal observation fixture".into(),
            },
            facts,
        )
    }

    #[test]
    fn recovery_receipt_accepts_delivered_continuations_only() {
        for (state, delivered, accepted) in [
            ("queued", true, true),
            ("delivering", true, true),
            ("queued", false, false),
            ("delivering", false, false),
            ("completed", true, false),
        ] {
            let store = TaskStore::in_memory().unwrap();
            let (input, facts) = fixture(&store);
            store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE queen_automation SET state=?1,
                 delivered_at=CASE WHEN ?2 THEN delivered_at ELSE NULL END WHERE id=1",
                    params![state, delivered],
                )
                .unwrap();
            let result =
                store.record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW);
            assert_eq!(
                result.is_ok(),
                accepted,
                "{state}, delivered={delivered}: {result:?}"
            );
        }
    }

    #[test]
    fn external_judgment_is_checked_bounded_and_valid_only_for_its_run_and_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let store = TaskStore::open(directory.path().join("external.sqlite3")).unwrap();
        let (mut input, mut facts) = fixture(&store);
        facts.activity = RecoveryTerminalActivity::Resting;
        input.reason = "Fictional upstream endpoint is unavailable".into();
        input.source = "Fictional endpoint status response".into();
        input.disposition = QueenRecoveryDisposition::VerifiedExternalWait {
            checked_evidence: " ".into(),
        };
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        input.disposition = QueenRecoveryDisposition::VerifiedExternalWait {
            checked_evidence: "Checked during this run: fixture returned service unavailable"
                .into(),
        };
        assert!(
            store
                .record_queen_recovery(
                    &input,
                    &facts,
                    &TaskActivityActor::worker(input.identity.worker_id),
                    NOW
                )
                .is_err()
        );
        facts.identity = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered {
                waiting_obligations: 1
            }
        ));
        assert!(matches!(
            store
                .queen_recovery_coverage(&TaskId::new().to_string(), &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing { .. }
        ));
        facts.identity.terminal_revision = Some("c".repeat(64));
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing { .. }
        ));
        assert_eq!(
            store.get_task(input.identity.task_id).unwrap().state,
            TaskState::Active
        );
    }

    #[test]
    fn receipt_survives_reopen_and_replays_without_another_activity_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let (input, facts) = fixture(&store);
        let saved = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        assert_ne!(saved.evidence_revision, input.identity.evidence_revision);
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        let mut current = facts;
        current.identity = saved.clone();
        assert_eq!(
            store
                .record_queen_recovery(&input, &current, &TaskActivityActor::operator(), NOW + 1)
                .unwrap(),
            saved
        );
        assert_eq!(
            store.get_task(saved.task_id).unwrap().state,
            TaskState::Active
        );
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row("SELECT count(*) FROM queen_recovery_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
        current.identity.terminal_revision = Some("c".repeat(64));
        assert!(
            store
                .record_queen_recovery(&input, &current, &TaskActivityActor::operator(), NOW + 2)
                .is_err()
        );
    }

    #[test]
    fn worker_and_invented_pending_delivery_cannot_certify_recovery() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        assert!(
            store
                .record_queen_recovery(
                    &input,
                    &facts,
                    &TaskActivityActor::worker(input.identity.worker_id),
                    NOW
                )
                .is_err()
        );
        input.disposition = QueenRecoveryDisposition::AwaitDelivery {
            message_id: "invented".into(),
        };
        facts.pending_message_id = Some("invented".into());
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row("SELECT count(*) FROM queen_recovery_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[test]
    fn completion_coverage_requires_fresh_complete_observations_not_a_saved_working_claim() {
        let store = TaskStore::in_memory().unwrap();
        let (input, mut facts) = fixture(&store);
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered { .. }
        ));
        let saved = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        facts.identity = saved;
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], false, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Unavailable
        ));
        facts.activity = RecoveryTerminalActivity::Resting;
        facts.identity.terminal_revision = Some("c".repeat(64));
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing {
                task_ids: vec![input.identity.task_id]
            }
        );
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing {
                task_ids: vec![input.identity.task_id]
            }
        );
    }

    #[test]
    fn finishing_without_observing_worker_recovery_closes_honestly_as_incomplete() {
        let store = TaskStore::in_memory().unwrap();
        let (input, _) = fixture(&store);
        assert!(matches!(
            store
                .finish_queen_automation_run(
                    &input.run_id,
                    swarm_domain::QueenAutomationOutcome::NoAction,
                    NOW
                )
                .unwrap(),
            crate::QueenAutomationFinish::Closed(swarm_domain::QueenAutomationOutcome::Incomplete)
        ));
    }

    #[test]
    fn finish_accepts_current_working_evidence_without_a_queen_approval_round_trip() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        assert!(matches!(
            store
                .finish_queen_automation_run_with_recovery(
                    &input.run_id,
                    swarm_domain::QueenAutomationOutcome::NoAction,
                    NOW,
                    &[facts],
                    true
                )
                .unwrap(),
            crate::QueenAutomationFinish::Closed(swarm_domain::QueenAutomationOutcome::NoAction)
        ));
    }

    #[test]
    fn duplicate_observations_cannot_certify_recovery_coverage() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone(), facts], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Unavailable
        );
    }

    #[test]
    fn delivered_request_invalidates_its_pending_delivery_assessment() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        let message = store
            .send_queen_worker_message(
                input.identity.task_id,
                input.identity.worker_id,
                swarm_domain::WorkerMessagePurpose::AssignedTask,
                "Continue this fixture task",
                NOW,
            )
            .unwrap();
        input.identity = store
            .queen_recovery_identity(&input.identity.attention_id)
            .unwrap()
            .unwrap();
        facts.identity = input.identity.clone();
        input.disposition = QueenRecoveryDisposition::AwaitDelivery {
            message_id: message.id.clone(),
        };
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_ok()
        );
        store
            .mark_task_message_delivered(&message.id, input.identity.session_id, NOW + 1)
            .unwrap();
        input.identity = store
            .queen_recovery_identity(&input.identity.attention_id)
            .unwrap()
            .unwrap();
        facts.identity = input.identity.clone();
        facts.pending_message_id = Some(message.id);
        let error = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW + 1)
            .unwrap_err();
        assert!(matches!(
            &error,
            TaskStoreError::RecoveryAssessmentRefused(_)
        ));
        assert!(
            error
                .to_string()
                .contains("A delivered message is not pending delivery")
        );
        assert!(!error.to_string().contains("database integrity"));
        assert_eq!(
            store.get_task(input.identity.task_id).unwrap().state,
            TaskState::Active
        );
    }

    #[test]
    fn corrupt_saved_receipts_remain_integrity_failures_not_command_refusals() {
        assert!(matches!(
            read_saved_recovery("not json"),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
        let store = TaskStore::in_memory().unwrap();
        let (mut input, _) = fixture(&store);
        input.run_id = "invalid stored run".into();
        assert!(matches!(
            read_saved_recovery(&serde_json::to_string(&input).unwrap()),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
    }

    #[test]
    fn capacity_refusal_is_atomic_and_retired_receipts_release_space() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        let mut retired = None;
        for index in 0..256 {
            let task = store
                .create_task(&format!("Capacity fixture {index}"), "/workspace/capacity")
                .unwrap();
            retired = Some(task.id);
            store.connection().unwrap().execute("INSERT INTO queen_recovery_receipts
                (task_id,run_id,attention_id,worker_id,session_id,accepted_revision,input_payload,recorded_at)
                VALUES (?1,?2,?3,?4,?5,?6,'{}',?7)", params![task.id.to_string(), input.run_id,
                format!("capacity-{index}"), input.identity.worker_id.to_string(), input.identity.session_id.to_string(),
                input.identity.evidence_revision, NOW]).unwrap();
        }
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        assert_eq!(
            store
                .queen_recovery_identity(&input.identity.attention_id)
                .unwrap()
                .unwrap()
                .evidence_revision,
            input.identity.evidence_revision
        );
        store
            .remove_task_as(
                retired.unwrap(),
                &TaskActivityActor::operator(),
                "Retire capacity fixture",
            )
            .unwrap();
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_ok()
        );
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row("SELECT count(*) FROM queen_recovery_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            256
        );
    }

    #[test]
    fn failed_activity_write_rolls_back_and_the_same_assessment_can_retry() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        store.connection().unwrap().execute_batch("CREATE TRIGGER fail_recovery_activity BEFORE INSERT ON task_activity BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        assert_eq!(
            store
                .queen_recovery_identity(&input.identity.attention_id)
                .unwrap()
                .unwrap()
                .evidence_revision,
            input.identity.evidence_revision
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_recovery_activity;")
            .unwrap();
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_ok()
        );
    }
}
