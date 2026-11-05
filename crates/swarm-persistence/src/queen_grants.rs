//! Whether Queen may grant a worker's command card, and her grant (ADR 0158).
//!
//! Off unless the operator turns it on. On, a command card whose asking
//! worker said none of the operator's reasons apply is Queen's to grant: it
//! leaves the operator's Needs you, wakes Queen, and her grant mints the same
//! grant the operator's Allow does (ADR 0129: exact text, once, in the session
//! that asked). A card naming any reason, or naming none at all, stays the
//! operator's, and so does one Queen hands to them.

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, DecisionRequestId, OperatorReason, WorkerId};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

/// The surface a Queen grant records, beside the operator's `inbox_action`.
pub const QUEEN_GRANT_SURFACE: &str = "queen_grant";

/// The longest note Queen's grant or hand-off may carry.
pub const MAX_QUEEN_GRANT_NOTE_BYTES: usize = 2_000;

/// Whether the decision aliased `$decision` is a command card that is Queen's
/// to grant right now. One definition, read by everything that decides whose
/// card it is: the operator's Needs you, its push notifications, who owns a
/// task's next move, and what wakes Queen. Pending is checked by the caller.
///
/// A card Queen filed is never hers: she does not grant a command she asked
/// for, so one of hers that left the operator's Needs you would wait on nobody.
macro_rules! queen_card {
    ($decision:literal) => {
        concat!(
            "(",
            $decision,
            ".requested_command IS NOT NULL AND ",
            $decision,
            ".operator_reasons IS '[]' AND ",
            $decision,
            ".queen_handoff_at IS NULL AND COALESCE((SELECT enabled FROM queen_grant_settings
                 WHERE singleton = 1), 0) = 1 AND NOT EXISTS (SELECT 1 FROM worker_profiles asker
                 WHERE asker.id = ",
            $decision,
            ".requesting_worker_id AND asker.role = 'queen'))"
        )
    };
}
pub(crate) use queen_card;

/// Adds the Hive-wide choice, and on each decision what the worker said of
/// the operator's reasons, which Queen granted it, and why she handed it on.
pub(crate) fn migrate_queen_grants(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_grant_settings (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
             updated_at INTEGER NOT NULL
         );",
    )?;
    for (column, definition) in [
        ("operator_reasons", "TEXT"),
        ("granted_by_queen", "TEXT"),
        ("queen_handoff_at", "INTEGER"),
        ("queen_handoff_reason", "TEXT"),
    ] {
        crate::add_column_once(
            transaction,
            "decision_requests",
            column,
            definition,
            version,
        )?;
    }
    Ok(())
}

fn enabled(connection: &Connection) -> Result<bool, TaskStoreError> {
    Ok(connection
        .query_row(
            "SELECT enabled FROM queen_grant_settings WHERE singleton = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

/// The stored form of what a worker said of the operator's reasons: a JSON
/// array of their names, sorted and without repeats, so `[]` is one string.
pub(crate) fn encode_reasons(reasons: &[OperatorReason]) -> String {
    let mut names: Vec<&str> = reasons.iter().map(|reason| reason.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    serde_json::Value::from(names).to_string()
}

pub(crate) fn decode_reasons(stored: Option<String>) -> Option<Vec<OperatorReason>> {
    let names: Vec<String> = serde_json::from_str(&stored?).ok()?;
    names
        .iter()
        .map(|name| OperatorReason::parse(name))
        .collect()
}

/// Whether the stored card `id` is Queen's to grant now.
pub(crate) fn is_queen_card(
    connection: &Connection,
    id: DecisionRequestId,
) -> Result<bool, TaskStoreError> {
    Ok(connection.query_row(
        concat!(
            "SELECT state = 'pending' AND ",
            queen_card!("d"),
            " FROM decision_requests d WHERE d.id = ?1"
        ),
        [id.to_string()],
        |row| row.get(0),
    )?)
}

/// What a pending card says about whether Queen may act on it, read in the
/// transaction that acts.
struct CardFacts {
    state: String,
    command: Option<String>,
    reasons: Option<String>,
    handed_off: bool,
    requester: String,
    urgency: String,
}

/// Only this Hive's cards, as the operator's Allow reads them.
fn card_facts(connection: &Connection, id: DecisionRequestId) -> Result<CardFacts, TaskStoreError> {
    connection
        .query_row(
            "SELECT d.state, d.requested_command, d.operator_reasons,
                    d.queen_handoff_at IS NOT NULL, d.requesting_worker_id, d.urgency
             FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
             WHERE d.id = ?1",
            [id.to_string()],
            |row| {
                Ok(CardFacts {
                    state: row.get(0)?,
                    command: row.get(1)?,
                    reasons: row.get(2)?,
                    handed_off: row.get(3)?,
                    requester: row.get(4)?,
                    urgency: row.get(5)?,
                })
            },
        )
        .optional()?
        .ok_or(TaskStoreError::DecisionNotFound)
}

/// Why Queen may not act on this card, or `None` when she may. Each answer
/// names what would make it hers, so a refusal is never a guess.
fn queen_refusal(facts: &CardFacts, queen: WorkerId, on: bool) -> Option<&'static str> {
    if facts.command.is_none() {
        return Some(
            "this decision asks to run no command; only a command card can be granted, and every other decision is the operator's",
        );
    }
    if !on {
        return Some(
            "the operator's \"Queen grants command cards\" setting is off, so every command card is theirs",
        );
    }
    if facts.requester == queen.to_string() {
        return Some(
            "this is your own card; Queen does not grant a command she asked for, so it is the operator's",
        );
    }
    if facts.handed_off {
        return Some("you handed this card to the operator; it is theirs now");
    }
    match facts.reasons.as_deref() {
        None => Some(
            "the asking worker did not say whether any of the operator's reasons apply (operator_only), so the card is theirs; the worker can withdraw it and ask again saying so",
        ),
        Some("[]") => None,
        Some(_) => Some(
            "the asking worker named one of the operator's reasons (credentials, permissions, spend over $25 or an irreversible delete), so the card is theirs",
        ),
    }
}

/// Only Queen grants or hands on a card. The service checks the caller's role
/// first; this holds the store to it too, so no other path can mint her grant.
fn require_queen(connection: &Connection, queen: WorkerId) -> Result<(), TaskStoreError> {
    let is_queen: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_profiles
                       WHERE id = ?1 AND role = 'queen' AND archived_at IS NULL)",
        [queen.to_string()],
        |row| row.get(0),
    )?;
    if is_queen {
        Ok(())
    } else {
        Err(TaskStoreError::QueenGrantRefused(
            "only Queen grants a command card or hands it to the operator",
        ))
    }
}

fn checked_note(note: &str) -> Result<&str, TaskStoreError> {
    let note = note.trim();
    if note.is_empty() || note.len() > MAX_QUEEN_GRANT_NOTE_BYTES {
        return Err(TaskStoreError::QueenGrantRefused(
            "say what you checked, in at most 2,000 bytes: the work you reviewed that this command carries out, and that none of the operator's reasons applies",
        ));
    }
    Ok(note)
}

impl TaskStore {
    /// Whether Queen may grant command cards.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn queen_grants(&self) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        enabled(&connection)
    }

    /// The operator choosing whether Queen may grant command cards. Cards
    /// already waiting follow it at once: turned off, every one is the
    /// operator's again.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn set_queen_grants(&self, enabled_now: bool, now: i64) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if enabled(&transaction)? != enabled_now {
            transaction.execute(
                "INSERT INTO queen_grant_settings (singleton, enabled, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET
                     enabled = excluded.enabled, updated_at = excluded.updated_at",
                params![enabled_now, now],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
            insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        }
        let stored = enabled(&transaction)?;
        transaction.commit()?;
        Ok(stored)
    }

    /// Files a command card that says which of the operator's reasons apply
    /// to it. An empty `reasons` says none does, which is what lets Queen
    /// grant it while the setting is on.
    ///
    /// # Errors
    /// Returns the same validation, capacity and identity errors as any
    /// decision request, and `InvalidCommandDecision` when it names no command.
    pub fn create_command_request(
        &self,
        request: &crate::NewDecisionRequest<'_>,
        reasons: &[OperatorReason],
    ) -> Result<swarm_domain::DecisionRequest, TaskStoreError> {
        if request.requested_command.is_none() {
            return Err(TaskStoreError::InvalidCommandDecision);
        }
        crate::decisions::create_with_reasons(self, request, &encode_reasons(reasons))
    }

    /// Queen grants a command card: the same grant the operator's Allow
    /// mints, recorded as hers. `note` is what she checked.
    ///
    /// ⚠️ NOT THE OPERATOR. `resolved_by_operator_id` stays empty and
    /// `granted_by_queen` names her, so nothing that asks whether the operator
    /// answered reads this as their ruling.
    ///
    /// # Errors
    /// `QueenGrantRefused` names why the card is not hers to grant;
    /// `DecisionAlreadyResolved` and `DecisionNotFound` as for the operator.
    pub fn grant_command_as_queen(
        &self,
        queen: WorkerId,
        id: DecisionRequestId,
        note: &str,
    ) -> Result<swarm_domain::DecisionRequest, TaskStoreError> {
        let note = checked_note(note)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        require_queen(&transaction, queen)?;
        let facts = card_facts(&transaction, id)?;
        if facts.state != "pending" {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        if let Some(refusal) = queen_refusal(&facts, queen, enabled(&transaction)?) {
            return Err(TaskStoreError::QueenGrantRefused(refusal));
        }
        let updated = transaction.execute(
            "UPDATE decision_requests SET
                 state = 'resolved', resolution_action = ?2, resolution_note = ?3,
                 resolution_surface = ?4, resolved_by_operator_id = NULL,
                 granted_by_queen = ?5,
                 resolved_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?1 AND state = 'pending'",
            params![
                id.to_string(),
                crate::decisions::GRANT_COMMAND_ACTION,
                note,
                QUEEN_GRANT_SURFACE,
                queen.to_string(),
            ],
        )?;
        if updated != 1 {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        crate::decisions::mint_command_grant(&transaction, id)?;
        crate::decision_clarification::cancel_queued(&transaction, id)?;
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

    /// Queen hands a card she will not grant to the operator, saying why. It
    /// joins their Needs you and notifies them as a new card would.
    ///
    /// # Errors
    /// `QueenGrantRefused` when the card is not hers; `DecisionAlreadyResolved`
    /// and `DecisionNotFound` as for the operator.
    pub fn hand_command_to_operator(
        &self,
        queen: WorkerId,
        id: DecisionRequestId,
        reason: &str,
        now: i64,
    ) -> Result<swarm_domain::DecisionRequest, TaskStoreError> {
        let reason = checked_note(reason)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        require_queen(&transaction, queen)?;
        let facts = card_facts(&transaction, id)?;
        if facts.state != "pending" {
            return Err(TaskStoreError::DecisionAlreadyResolved);
        }
        if let Some(refusal) = queen_refusal(&facts, queen, enabled(&transaction)?) {
            return Err(TaskStoreError::QueenGrantRefused(refusal));
        }
        transaction.execute(
            "UPDATE decision_requests SET queen_handoff_at = ?2, queen_handoff_reason = ?3,
                 updated_at = unixepoch()
             WHERE id = ?1 AND state = 'pending' AND queen_handoff_at IS NULL",
            params![id.to_string(), now, reason],
        )?;
        let urgency = facts
            .urgency
            .parse()
            .map_err(|_| TaskStoreError::IntegrityFailure("decision urgency".to_owned()))?;
        crate::notifications::enqueue_decision_notifications(&transaction, id, urgency, now)?;
        insert_control_room_event(&transaction, ControlRoomEventKind::DecisionsChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_decision_request(id)
    }

    /// The command cards waiting for Queen, oldest first.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn queen_command_cards(
        &self,
    ) -> Result<Vec<swarm_domain::DecisionRequest>, TaskStoreError> {
        let connection = self.connection()?;
        Ok(crate::decisions::read_decisions(
            &connection,
            &format!(
                "{} FROM decision_requests d \
                 WHERE d.state = 'pending' AND d.hive_id = \
                     (SELECT hive_id FROM local_hive_identity WHERE singleton = 1) AND {} \
                 ORDER BY d.created_at, d.id LIMIT 50",
                crate::decisions::DECISION_COLUMNS,
                queen_card!("d")
            ),
            [],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        DecisionRequestKind, DecisionUrgency, NextMoveOwner, OperatorReason, ProviderKind, TaskId,
        WorkerId, WorkerSessionId,
    };

    use super::{decode_reasons, encode_reasons};
    use crate::decisions::GRANT_COMMAND_ACTION;
    use crate::{NewDecisionRequest, TaskStore, TaskStoreError};

    const RELEASE: &str = "scripts/release.sh --sha 0123456789abcdef";

    fn card(worker: WorkerId, task: TaskId) -> NewDecisionRequest<'static> {
        NewDecisionRequest {
            requesting_worker_id: worker,
            task_id: Some(task),
            kind: DecisionRequestKind::Approval,
            urgency: DecisionUrgency::Normal,
            title: "Run the reviewed release once",
            summary: "One release of the reviewed work.",
            reason: "Queen reviewed the work and said to release it.",
            risk: "",
            evidence: "",
            suggested_action: "",
            allowed_actions: &[],
            operator_actions: &[],
            questions: &[],
            deadline: None,
            requested_command: Some(RELEASE),
            requested_script: None,
        }
    }

    struct Hive {
        store: TaskStore,
        queen: WorkerId,
        worker: WorkerId,
        task: TaskId,
    }

    fn hive() -> Hive {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap().id;
        store
            .bind_worker_session(queen, WorkerSessionId::new())
            .unwrap();
        let worker = store
            .create_worker(
                "Shop",
                ProviderKind::ClaudeCode,
                "/workspace/shop",
                false,
                1,
            )
            .unwrap()
            .id;
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker, session).unwrap();
        let task = store
            .create_task("Release the shop", "/workspace/shop")
            .unwrap()
            .id;
        store
            .transition_task(task, swarm_domain::TaskState::Ready)
            .unwrap();
        store.assign_task(task, session).unwrap();
        Hive {
            store,
            queen,
            worker,
            task,
        }
    }

    /// Whether the operator's Needs you lists this decision, read the way the
    /// badge and the push notifications read it.
    fn in_needs_you(store: &TaskStore, id: swarm_domain::DecisionRequestId) -> bool {
        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        crate::attention::needs_you_subjects(&transaction, 0)
            .unwrap()
            .iter()
            .any(|subject| subject.decision_id() == Some(id.to_string().as_str()))
    }

    fn wakes_queen(store: &TaskStore) -> i64 {
        let connection = store.connection().unwrap();
        crate::queen_conductor::actionable_count(&connection, 0).unwrap()
    }

    fn owner(hive: &Hive) -> NextMoveOwner {
        hive.store.get_task(hive.task).unwrap().next_move_owner
    }

    fn use_grant(store: &TaskStore, worker: WorkerId) -> bool {
        store
            .live_command_grant(worker, RELEASE)
            .unwrap()
            .is_some_and(|grant| store.spend_command_grant(grant.decision_id).unwrap())
    }

    fn refusal(result: Result<swarm_domain::DecisionRequest, TaskStoreError>) -> String {
        match result {
            Err(TaskStoreError::QueenGrantRefused(why)) => why.to_owned(),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn reasons_are_stored_as_one_sorted_list() {
        assert_eq!(encode_reasons(&[]), "[]");
        assert_eq!(
            encode_reasons(&[
                OperatorReason::SpendOver25Usd,
                OperatorReason::Credentials,
                OperatorReason::Credentials,
            ]),
            r#"["credentials","spend_over_25_usd"]"#
        );
        assert_eq!(
            decode_reasons(Some(r#"["credentials","spend_over_25_usd"]"#.to_owned())),
            Some(vec![
                OperatorReason::Credentials,
                OperatorReason::SpendOver25Usd
            ])
        );
        assert_eq!(decode_reasons(None), None);
        assert_eq!(
            decode_reasons(Some(r#"["anything"]"#.to_owned())),
            None,
            "an unknown name is not read as no reason"
        );
    }

    /// The setting decides, at every read: a card is Queen's only while it is
    /// on, and goes back to the operator the moment it is off.
    #[test]
    fn a_card_with_no_operator_reason_is_queens_only_while_the_operator_lets_her() {
        let hive = hive();
        let filed = hive
            .store
            .create_command_request(&card(hive.worker, hive.task), &[])
            .unwrap();
        assert_eq!(filed.operator_reasons, Some(Vec::new()));
        assert!(!filed.awaiting_queen, "off: the operator's card");
        assert!(in_needs_you(&hive.store, filed.id));
        assert_eq!(owner(&hive), NextMoveOwner::Operator);
        let before = wakes_queen(&hive.store);
        assert!(
            refusal(
                hive.store
                    .grant_command_as_queen(hive.queen, filed.id, "Reviewed.")
            )
            .contains("setting is off")
        );

        assert!(hive.store.set_queen_grants(true, 10).unwrap());
        let card = hive.store.get_decision_request(filed.id).unwrap();
        assert!(card.awaiting_queen, "on: Queen's card");
        assert!(
            !in_needs_you(&hive.store, filed.id),
            "not the operator's to answer"
        );
        assert_eq!(
            owner(&hive),
            NextMoveOwner::Worker,
            "not the operator's move"
        );
        assert_eq!(wakes_queen(&hive.store), before + 1, "it wakes Queen");
        assert_eq!(
            hive.store
                .queen_command_cards()
                .unwrap()
                .iter()
                .map(|card| card.id)
                .collect::<Vec<_>>(),
            vec![filed.id]
        );

        assert!(!hive.store.set_queen_grants(false, 20).unwrap());
        assert!(
            !hive
                .store
                .get_decision_request(filed.id)
                .unwrap()
                .awaiting_queen
        );
        assert!(
            in_needs_you(&hive.store, filed.id),
            "off again: the operator's again"
        );
        assert_eq!(owner(&hive), NextMoveOwner::Operator);
    }

    /// A reason the worker names, or a card that names nothing, stays with
    /// the operator whatever the setting says.
    #[test]
    fn a_named_reason_or_none_said_keeps_the_card_with_the_operator() {
        let hive = hive();
        hive.store.set_queen_grants(true, 10).unwrap();
        let reasoned = hive
            .store
            .create_command_request(
                &card(hive.worker, hive.task),
                &[OperatorReason::Permissions],
            )
            .unwrap();
        let silent = hive
            .store
            .create_decision_request(&card(hive.worker, hive.task))
            .unwrap();
        let own = hive
            .store
            .create_command_request(&card(hive.queen, hive.task), &[])
            .unwrap();

        // Queen's own card says no reason applies, and is still the
        // operator's: she does not grant a command she asked for, so a card
        // of hers that left their Needs you would wait on nobody.
        for (id, why) in [
            (reasoned.id, "named one of the operator's reasons"),
            (silent.id, "did not say"),
            (own.id, "your own card"),
        ] {
            let card = hive.store.get_decision_request(id).unwrap();
            assert!(!card.awaiting_queen, "{why}");
            assert!(in_needs_you(&hive.store, id), "{why}");
            assert!(
                refusal(
                    hive.store
                        .grant_command_as_queen(hive.queen, id, "Reviewed.")
                )
                .contains(why)
            );
        }
        assert!(
            hive.store.queen_command_cards().unwrap().is_empty(),
            "none of them is listed for Queen"
        );
        assert!(!use_grant(&hive.store, hive.worker), "nothing was granted");
    }

    /// The workers list says a worker awaits the operator only while its card
    /// is theirs: one waiting on Queen awaits her, until she hands it on.
    #[test]
    fn a_worker_waiting_on_queen_is_not_awaiting_the_operator() {
        let hive = hive();
        hive.store.set_queen_grants(true, 10).unwrap();
        let filed = hive
            .store
            .create_command_request(&card(hive.worker, hive.task), &[])
            .unwrap();
        assert!(filed.awaiting_queen);
        assert!(
            !hive
                .store
                .workers_awaiting_operator()
                .unwrap()
                .contains(&hive.worker)
        );
        hive.store
            .hand_command_to_operator(hive.queen, filed.id, "Theirs.", 20)
            .unwrap();
        assert!(
            hive.store
                .workers_awaiting_operator()
                .unwrap()
                .contains(&hive.worker)
        );
    }

    /// Queen acts only on this Hive's cards, as the operator's Allow does.
    #[test]
    fn queen_acts_only_on_this_hives_cards() {
        let hive = hive();
        hive.store.set_queen_grants(true, 10).unwrap();
        let filed = hive
            .store
            .create_command_request(&card(hive.worker, hive.task), &[])
            .unwrap();
        {
            let connection = hive.store.connection().unwrap();
            let operator = swarm_domain::OperatorId::new().to_string();
            let elsewhere = swarm_domain::HiveId::new().to_string();
            connection
                .execute(
                    "INSERT INTO operators (id, display_name) VALUES (?1, 'Guest')",
                    [&operator],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id) VALUES (?1, 'Guest Hive', ?2)",
                    [&elsewhere, &operator],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE decision_requests SET hive_id = ?2 WHERE id = ?1",
                    [filed.id.to_string(), elsewhere],
                )
                .unwrap();
        }
        assert!(hive.store.queen_command_cards().unwrap().is_empty());
        assert!(matches!(
            hive.store
                .grant_command_as_queen(hive.queen, filed.id, "Reviewed."),
            Err(TaskStoreError::DecisionNotFound)
        ));
        assert!(matches!(
            hive.store
                .hand_command_to_operator(hive.queen, filed.id, "Theirs.", 20),
            Err(TaskStoreError::DecisionNotFound)
        ));
        assert!(!use_grant(&hive.store, hive.worker), "nothing was granted");
    }

    /// Only Queen grants or hands on a card, whatever the caller: a worker
    /// cannot mint another worker's grant through the store.
    #[test]
    fn only_queen_grants_or_hands_on_a_card() {
        let hive = hive();
        hive.store.set_queen_grants(true, 10).unwrap();
        let other = hive
            .store
            .create_worker(
                "Till",
                ProviderKind::ClaudeCode,
                "/workspace/till",
                false,
                1,
            )
            .unwrap()
            .id;
        let filed = hive
            .store
            .create_command_request(&card(hive.worker, hive.task), &[])
            .unwrap();
        assert!(filed.awaiting_queen);
        for granter in [other, hive.worker] {
            assert!(
                refusal(
                    hive.store
                        .grant_command_as_queen(granter, filed.id, "Reviewed.")
                )
                .contains("only Queen")
            );
            assert!(
                refusal(
                    hive.store
                        .hand_command_to_operator(granter, filed.id, "Theirs.", 20)
                )
                .contains("only Queen")
            );
        }
        assert!(!use_grant(&hive.store, hive.worker), "nothing was granted");
        assert!(
            hive.store
                .get_decision_request(filed.id)
                .unwrap()
                .awaiting_queen,
            "the card is still Queen's"
        );
    }

    /// Queen's grant is the operator's grant in every way but whose it is:
    /// exact, once, and recorded as hers rather than as an operator ruling.
    #[test]
    fn queens_grant_runs_once_and_is_recorded_as_hers() {
        let hive = hive();
        hive.store.set_queen_grants(true, 10).unwrap();
        let filed = hive
            .store
            .create_command_request(&card(hive.worker, hive.task), &[])
            .unwrap();
        assert!(
            refusal(
                hive.store
                    .grant_command_as_queen(hive.queen, filed.id, "   ")
            )
            .contains("say what you checked"),
            "a grant says what it rests on"
        );

        let granted = hive
            .store
            .grant_command_as_queen(
                hive.queen,
                filed.id,
                "I reviewed the release; no reason applies.",
            )
            .unwrap();
        assert_eq!(
            granted.resolution_action.as_deref(),
            Some(GRANT_COMMAND_ACTION)
        );
        assert_eq!(granted.granted_by_queen, Some(hive.queen));
        assert_eq!(
            granted.resolved_by_operator_id, None,
            "not the operator's ruling"
        );
        assert_eq!(granted.resolution_surface, super::QUEEN_GRANT_SURFACE);
        assert!(!granted.awaiting_queen);
        assert!(use_grant(&hive.store, hive.worker), "the command runs");
        assert!(!use_grant(&hive.store, hive.worker), "once");
        assert!(matches!(
            hive.store
                .grant_command_as_queen(hive.queen, filed.id, "Again."),
            Err(TaskStoreError::DecisionAlreadyResolved)
        ));
    }

    /// A card Queen will not grant goes to the operator with her reason, and
    /// she cannot then grant it after all.
    #[test]
    fn a_card_queen_hands_on_is_the_operators() {
        let hive = hive();
        hive.store.set_queen_grants(true, 10).unwrap();
        let filed = hive
            .store
            .create_command_request(&card(hive.worker, hive.task), &[])
            .unwrap();
        let handed = hive
            .store
            .hand_command_to_operator(hive.queen, filed.id, "It deletes a storage account.", 30)
            .unwrap();
        assert!(!handed.awaiting_queen);
        assert_eq!(
            handed.queen_handoff_reason.as_deref(),
            Some("It deletes a storage account.")
        );
        assert!(in_needs_you(&hive.store, filed.id));
        assert_eq!(owner(&hive), NextMoveOwner::Operator);
        assert!(
            refusal(
                hive.store
                    .grant_command_as_queen(hive.queen, filed.id, "Reviewed.")
            )
            .contains("you handed this card")
        );
        hive.store
            .resolve_decision_request(filed.id, GRANT_COMMAND_ACTION, "", "inbox")
            .unwrap();
        assert!(
            use_grant(&hive.store, hive.worker),
            "the operator can still allow it"
        );
    }
}
