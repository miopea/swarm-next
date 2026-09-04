//! One current startup receipt per worker, owned by its immutable session binding.
use crate::{TaskStore, TaskStoreError, insert_control_room_event};
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, ConversationRecovery, ConversationRecoveryAttempt,
    ConversationRecoveryState, ProviderConversationId, ProviderSessionStartKind, WorkerSessionId,
};

pub(super) fn migrate(tx: &Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::CONVERSATION_RECOVERY_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch(
        "CREATE TABLE worker_startup_context (
        worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
        session_id TEXT NOT NULL UNIQUE,
        selected_conversation TEXT,
        status TEXT NOT NULL CHECK(status IN ('pending','settled','canceled')),
        outcome TEXT CHECK(outcome IS NULL OR length(CAST(outcome AS BLOB)) <= 1024)
    );",
    )?;
    // Existing sessions have no trustworthy startup selection snapshot. Do not
    // manufacture one from today's pin during migration.
    tx.pragma_update(
        None,
        "user_version",
        crate::CONVERSATION_RECOVERY_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Confirms engine selections only when their exact revision and conversation
    /// are already the durable default of the current binding. A manual fence is
    /// not a provider selection, even when it advanced the stored revision.
    /// # Errors
    /// Returns storage errors or rejects more than 256 candidates.
    pub fn confirmed_provider_selections(
        &self,
        candidates: &[(WorkerSessionId, swarm_domain::ProviderConversationSelection)],
    ) -> Result<
        std::collections::HashMap<WorkerSessionId, swarm_domain::ProviderConversationSelection>,
        TaskStoreError,
    > {
        if candidates.len() > 256 {
            return Err(TaskStoreError::IntegrityFailure(
                "selection projection limit exceeded".into(),
            ));
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT EXISTS(SELECT 1 FROM worker_startup_context context
             JOIN worker_profiles worker ON worker.id = context.worker_id
             JOIN worker_sessions session ON session.worker_id = worker.id AND session.session_id = context.session_id
             WHERE context.session_id = ?1 AND context.selection_revision = ?2
               AND context.selection_suspended = 0 AND worker.provider_conversation_id = ?3
               AND worker.provider = 'claude_code' AND worker.archived_at IS NULL AND session.ended_at IS NULL)"
        )?;
        let mut confirmed = std::collections::HashMap::new();
        for (session, selection) in candidates {
            let Ok(revision) = i64::try_from(selection.revision) else {
                continue;
            };
            if revision > 1
                && statement.query_row(
                    params![
                        session.to_string(),
                        revision,
                        selection.conversation.to_string()
                    ],
                    |row| row.get::<_, bool>(0),
                )?
            {
                confirmed.insert(*session, *selection);
            }
        }
        Ok(confirmed)
    }

    /// Whether this current binding has a receipt that can order an operator fence.
    /// Older sessions without a startup receipt must retain manual-only selection.
    /// # Errors
    /// Returns storage errors rather than treating unavailable evidence as ready.
    pub fn provider_selection_fence_ready(
        &self,
        worker: swarm_domain::WorkerId,
        session: WorkerSessionId,
    ) -> Result<bool, TaskStoreError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_startup_context context
             JOIN worker_profiles worker ON worker.id = context.worker_id
             JOIN worker_sessions session ON session.worker_id = worker.id AND session.session_id = context.session_id
             WHERE context.worker_id = ?1 AND context.session_id = ?2
               AND worker.archived_at IS NULL AND session.ended_at IS NULL
               AND worker.provider = 'claude_code')",
            params![worker.to_string(), session.to_string()],
            |row| row.get(0),
        )?)
    }

    /// Applies only a newer paired interactive selection to its still-bound worker.
    /// Startup revision one cannot bypass recovery policy. Explicit unfenced
    /// choices suspend this consumer until a new binding or fenced choice.
    /// # Errors
    /// Returns errors for invalid revisions or persistence/event failures.
    pub fn reconcile_provider_selection(
        &self,
        session: WorkerSessionId,
        selection: swarm_domain::ProviderConversationSelection,
    ) -> Result<bool, TaskStoreError> {
        let revision = i64::try_from(selection.revision)
            .map_err(|_| TaskStoreError::IntegrityFailure("invalid selection revision".into()))?;
        if revision <= 1 {
            return Ok(false);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let worker: Option<String> = tx.query_row(
            "SELECT context.worker_id FROM worker_startup_context context
             JOIN worker_profiles worker ON worker.id = context.worker_id
             JOIN worker_sessions session ON session.worker_id = worker.id AND session.session_id = context.session_id
             WHERE context.session_id = ?1 AND context.selection_revision < ?2
               AND context.selection_suspended = 0 AND worker.archived_at IS NULL
               AND session.ended_at IS NULL AND worker.provider = 'claude_code'",
            params![session.to_string(), revision], |row| row.get(0)).optional()?;
        let Some(worker) = worker else {
            return Ok(false);
        };
        tx.execute("UPDATE worker_startup_context SET selection_revision = ?2,
                    status = CASE WHEN status = 'pending' THEN 'canceled' ELSE status END WHERE worker_id = ?1", params![worker, revision])?;
        tx.execute("UPDATE worker_profiles SET provider_conversation_id = ?2, updated_at = unixepoch() WHERE id = ?1", params![worker, selection.conversation.to_string()])?;
        insert_control_room_event(&tx, ControlRoomEventKind::WorkersChanged)?;
        tx.commit()?;
        Ok(true)
    }

    /// Reads settled startup outcomes only for the requested still-bound sessions.
    /// This is a bounded projection, not a scan of historical worker activity.
    ///
    /// # Errors
    /// Returns an error for more than 256 sessions, corrupt outcomes or storage failure.
    pub fn provider_recovery_outcomes(
        &self,
        sessions: &[WorkerSessionId],
    ) -> Result<std::collections::HashMap<WorkerSessionId, ConversationRecoveryState>, TaskStoreError>
    {
        if sessions.len() > 256 {
            return Err(TaskStoreError::IntegrityFailure(
                "recovery projection limit exceeded".into(),
            ));
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT context.outcome FROM worker_startup_context context
             JOIN worker_profiles worker ON worker.id = context.worker_id
             JOIN worker_sessions session ON session.worker_id = worker.id AND session.session_id = context.session_id
             WHERE context.session_id = ?1 AND context.status = 'settled'
               AND worker.archived_at IS NULL AND session.ended_at IS NULL")?;
        let mut outcomes = std::collections::HashMap::new();
        for session in sessions {
            let payload: Option<String> = statement
                .query_row([session.to_string()], |row| row.get(0))
                .optional()?;
            if let Some(payload) = payload {
                let outcome: ConversationRecoveryState =
                    serde_json::from_str(&payload).map_err(|_| {
                        TaskStoreError::IntegrityFailure("invalid stored recovery outcome".into())
                    })?;
                if matches!(outcome, ConversationRecoveryState::Attempt { .. }) {
                    return Err(TaskStoreError::IntegrityFailure(
                        "unsettled recovery outcome".into(),
                    ));
                }
                outcomes.insert(*session, outcome);
            }
        }
        Ok(outcomes)
    }

    /// Settles authenticated startup evidence against the current durable binding.
    /// Returns None for missing, canceled, obsolete, duplicate or invalid evidence.
    /// No task is replayed and no provider is changed by this transaction.
    ///
    /// # Errors
    /// Returns persistence errors; outcome, pin and activity roll back together.
    pub fn reconcile_provider_start(
        &self,
        session: WorkerSessionId,
        attempt: ConversationRecoveryAttempt,
        kind: ProviderSessionStartKind,
        conversation: ProviderConversationId,
    ) -> Result<Option<ConversationRecoveryState>, TaskStoreError> {
        let Some(mut recovery) = ConversationRecovery::from_attempt(attempt) else {
            return Ok(None);
        };
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let worker: Option<String> = tx.query_row(
            "SELECT context.worker_id FROM worker_startup_context context
             JOIN worker_profiles worker ON worker.id = context.worker_id
             JOIN worker_sessions session ON session.worker_id = worker.id AND session.session_id = context.session_id
             WHERE context.session_id = ?1 AND context.status = 'pending'
               AND worker.archived_at IS NULL AND session.ended_at IS NULL
               AND worker.provider = 'claude_code'
               AND worker.provider_conversation_id IS context.selected_conversation",
            [session.to_string()], |row| row.get(0)).optional()?;
        let Some(worker) = worker else {
            return Ok(None);
        };
        if !recovery.observe_provider_start(session, session, attempt, kind, conversation) {
            return Ok(None);
        }
        let outcome = recovery.state();
        let payload = serde_json::to_string(&outcome)
            .map_err(|_| TaskStoreError::IntegrityFailure("invalid recovery outcome".into()))?;
        tx.execute(
            "UPDATE worker_startup_context SET status = 'settled', outcome = ?2
                    WHERE worker_id = ?1",
            params![worker, payload],
        )?;
        if let ConversationRecoveryState::Restored { conversation, .. }
        | ConversationRecoveryState::Fresh { conversation } = outcome
        {
            tx.execute(
                "UPDATE worker_profiles SET provider_conversation_id = ?2, updated_at = unixepoch()
                        WHERE id = ?1",
                params![worker, conversation.to_string()],
            )?;
        }
        insert_control_room_event(&tx, ControlRoomEventKind::WorkersChanged)?;
        tx.commit()?;
        Ok(Some(outcome))
    }
}

pub(super) fn migrate_selection(tx: &Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::CONVERSATION_SELECTION_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch("ALTER TABLE worker_startup_context ADD COLUMN selection_revision INTEGER NOT NULL DEFAULT 0 CHECK(selection_revision >= 0);
        ALTER TABLE worker_startup_context ADD COLUMN selection_suspended INTEGER NOT NULL DEFAULT 0 CHECK(selection_suspended IN (0,1));
        UPDATE worker_startup_context SET selection_suspended = 1;")?;
    tx.pragma_update(
        None,
        "user_version",
        crate::CONVERSATION_SELECTION_SCHEMA_VERSION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    #[test]
    fn selection_projection_requires_committed_exact_evidence_and_rejects_fences() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let selected = swarm_domain::ProviderConversationSelection {
            revision: 2,
            conversation: ProviderConversationId::new(),
        };
        let candidates = [(session, selected)];
        assert!(
            store
                .confirmed_provider_selections(&candidates)
                .unwrap()
                .is_empty()
        );
        store
            .reconcile_provider_selection(session, selected)
            .unwrap();
        assert_eq!(
            store
                .confirmed_provider_selections(&candidates)
                .unwrap()
                .get(&session),
            Some(&selected)
        );
        let wrong = [(
            session,
            swarm_domain::ProviderConversationSelection {
                conversation: ProviderConversationId::new(),
                ..selected
            },
        )];
        assert!(
            store
                .confirmed_provider_selections(&wrong)
                .unwrap()
                .is_empty()
        );
        store
            .repoint_provider_conversation_fenced(
                worker.id,
                &selected.conversation,
                Some((session, 3)),
            )
            .unwrap();
        assert!(
            store
                .confirmed_provider_selections(&candidates)
                .unwrap()
                .is_empty()
        );
        let later = [(
            session,
            swarm_domain::ProviderConversationSelection {
                revision: 4,
                ..selected
            },
        )];
        store
            .reconcile_provider_selection(session, later[0].1)
            .unwrap();
        assert_eq!(
            store.confirmed_provider_selections(&later).unwrap().len(),
            1
        );
        store
            .repoint_provider_conversation(worker.id, &selected.conversation)
            .unwrap();
        assert!(
            store
                .confirmed_provider_selections(&later)
                .unwrap()
                .is_empty()
        );
        store.release_worker_session(session).unwrap();
        assert!(
            store
                .confirmed_provider_selections(&later)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .confirmed_provider_selections(&vec![(session, selected); 257])
                .is_err()
        );
    }

    #[test]
    fn fence_readiness_requires_a_current_receipt_not_just_a_live_binding() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        assert!(
            !store
                .provider_selection_fence_ready(worker.id, session)
                .unwrap()
        );
        store.bind_worker_session(worker.id, session).unwrap();
        assert!(
            store
                .provider_selection_fence_ready(worker.id, session)
                .unwrap()
        );
        assert!(
            !store
                .provider_selection_fence_ready(worker.id, WorkerSessionId::new())
                .unwrap()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "DELETE FROM worker_startup_context WHERE worker_id = ?1",
                [worker.id.to_string()],
            )
            .unwrap();
        assert!(
            !store
                .provider_selection_fence_ready(worker.id, session)
                .unwrap()
        );
        let manual = ProviderConversationId::new();
        store
            .repoint_provider_conversation(worker.id, &manual)
            .unwrap();
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            Some(manual)
        );
        store.release_worker_session(session).unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        assert!(
            store
                .provider_selection_fence_ready(worker.id, session)
                .unwrap()
        );
        store.release_worker_session(session).unwrap();
        assert!(
            !store
                .provider_selection_fence_ready(worker.id, session)
                .unwrap()
        );
    }

    #[test]
    fn selection_revisions_preserve_manual_fences_and_resume_following_after_them() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let selected = |revision| swarm_domain::ProviderConversationSelection {
            revision,
            conversation: ProviderConversationId::new(),
        };
        assert!(
            !store
                .reconcile_provider_selection(session, selected(1))
                .unwrap()
        );
        let first = selected(2);
        assert!(store.reconcile_provider_selection(session, first).unwrap());
        assert!(
            !store
                .reconcile_provider_selection(session, selected(2))
                .unwrap()
        );
        let manual = ProviderConversationId::new();
        store
            .repoint_provider_conversation_fenced(worker.id, &manual, Some((session, 4)))
            .unwrap();
        assert!(
            !store
                .reconcile_provider_selection(session, selected(3))
                .unwrap()
        );
        assert!(
            !store
                .reconcile_provider_selection(session, selected(4))
                .unwrap()
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            Some(manual)
        );
        let later = selected(5);
        assert!(store.reconcile_provider_selection(session, later).unwrap());
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            Some(later.conversation)
        );
        store
            .repoint_provider_conversation(worker.id, &manual)
            .unwrap();
        assert!(
            !store
                .reconcile_provider_selection(session, selected(6))
                .unwrap()
        );
        store.release_worker_session(session).unwrap();
        let next = WorkerSessionId::new();
        store.bind_worker_session(worker.id, next).unwrap();
        assert!(
            !store
                .reconcile_provider_selection(session, selected(7))
                .unwrap()
        );
        assert!(
            store
                .reconcile_provider_selection(next, selected(2))
                .unwrap()
        );
    }

    #[test]
    fn failed_selection_event_and_wrong_session_fence_do_not_change_the_default() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let initial = store
            .get_worker_profile(worker.id)
            .unwrap()
            .provider_conversation_id;
        let selection = swarm_domain::ProviderConversationSelection {
            revision: 2,
            conversation: ProviderConversationId::new(),
        };
        assert!(
            store
                .repoint_provider_conversation_fenced(
                    worker.id,
                    &selection.conversation,
                    Some((WorkerSessionId::new(), 3))
                )
                .is_err()
        );
        store.connection().unwrap().execute_batch("CREATE TRIGGER reject_selection_event BEFORE INSERT ON control_room_events BEGIN SELECT RAISE(ABORT, 'test'); END;").unwrap();
        assert!(
            store
                .reconcile_provider_selection(session, selection)
                .is_err()
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            initial
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_selection_event;")
            .unwrap();
        assert!(
            store
                .reconcile_provider_selection(session, selection)
                .unwrap()
        );
    }

    fn attempt() -> ConversationRecoveryAttempt {
        let ConversationRecoveryState::Attempt { attempt } =
            ConversationRecovery::new(None, true).state()
        else {
            panic!("attempt");
        };
        attempt
    }

    #[test]
    fn selection_migration_preserves_prior_manual_override() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("selection.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store
            .reconcile_provider_start(
                session,
                attempt(),
                ProviderSessionStartKind::Resumed,
                ProviderConversationId::new(),
            )
            .unwrap();
        let manual = ProviderConversationId::new();
        store
            .repoint_provider_conversation(worker.id, &manual)
            .unwrap();
        store.connection().unwrap().execute_batch("DROP TABLE operator_submissions; DROP TABLE operator_statement_resolutions; DROP TABLE operator_statements; ALTER TABLE task_dispatches DROP COLUMN generation; ALTER TABLE worker_startup_context DROP COLUMN selection_revision; ALTER TABLE worker_startup_context DROP COLUMN selection_suspended; PRAGMA user_version = 127;").unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert!(
            !store
                .reconcile_provider_selection(
                    session,
                    swarm_domain::ProviderConversationSelection {
                        revision: 2,
                        conversation: ProviderConversationId::new()
                    }
                )
                .unwrap()
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            Some(manual)
        );
    }

    #[test]
    fn outcome_projection_is_bounded_and_rejects_corruption_or_ended_binding() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        assert!(
            store
                .provider_recovery_outcomes(&[session])
                .unwrap()
                .is_empty()
        );
        store
            .reconcile_provider_start(
                session,
                attempt(),
                ProviderSessionStartKind::Resumed,
                ProviderConversationId::new(),
            )
            .unwrap();
        assert!(
            store
                .provider_recovery_outcomes(&[WorkerSessionId::new()])
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .provider_recovery_outcomes(&vec![session; 257])
                .is_err()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_startup_context SET outcome = 'invalid' WHERE session_id = ?1",
                [session.to_string()],
            )
            .unwrap();
        assert!(store.provider_recovery_outcomes(&[session]).is_err());
        store.release_worker_session(session).unwrap();
        assert!(
            store
                .provider_recovery_outcomes(&[session])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn migration_does_not_guess_context_for_existing_sessions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("old.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let original = store
            .get_worker_profile(worker.id)
            .unwrap()
            .provider_conversation_id;
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE operator_submissions; DROP TABLE operator_statement_resolutions; DROP TABLE operator_statements; ALTER TABLE task_dispatches DROP COLUMN generation; DROP TABLE worker_startup_context; PRAGMA user_version = 126;")
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store
                .reconcile_provider_start(
                    session,
                    attempt(),
                    ProviderSessionStartKind::Resumed,
                    ProviderConversationId::new()
                )
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            original
        );
    }

    #[test]
    fn startup_reconciliation_persists_once_and_preserves_binding() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let chosen = ProviderConversationId::new();
        let attempt = attempt();
        assert!(matches!(
            store
                .reconcile_provider_start(
                    session,
                    attempt,
                    ProviderSessionStartKind::Resumed,
                    chosen
                )
                .unwrap(),
            Some(ConversationRecoveryState::Restored {
                via_continue: true,
                ..
            })
        ));
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            Some(chosen)
        );
        assert!(matches!(
            store
                .provider_recovery_outcomes(&[session])
                .unwrap()
                .get(&session),
            Some(ConversationRecoveryState::Restored { .. })
        ));
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .active_session_id,
            Some(session)
        );
        assert_eq!(
            store
                .reconcile_provider_start(
                    session,
                    attempt,
                    ProviderSessionStartKind::Resumed,
                    ProviderConversationId::new()
                )
                .unwrap(),
            None
        );
    }

    #[test]
    fn startup_reconciliation_cannot_undo_operator_choice_or_replace_session() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let original = ProviderConversationId::new();
        store
            .repoint_provider_conversation(worker.id, &original)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store
            .repoint_provider_conversation(worker.id, &ProviderConversationId::new())
            .unwrap();
        store
            .repoint_provider_conversation(worker.id, &original)
            .unwrap();
        assert_eq!(
            store
                .reconcile_provider_start(
                    session,
                    attempt(),
                    ProviderSessionStartKind::Resumed,
                    ProviderConversationId::new()
                )
                .unwrap(),
            None
        );
        store.release_worker_session(session).unwrap();
        let next = WorkerSessionId::new();
        store.bind_worker_session(worker.id, next).unwrap();
        assert_eq!(
            store
                .reconcile_provider_start(
                    session,
                    attempt(),
                    ProviderSessionStartKind::Resumed,
                    ProviderConversationId::new()
                )
                .unwrap(),
            None
        );
        // New context cannot masquerade as a successful continuation.
        assert!(matches!(
            store
                .reconcile_provider_start(
                    next,
                    attempt(),
                    ProviderSessionStartKind::New,
                    ProviderConversationId::new()
                )
                .unwrap(),
            Some(ConversationRecoveryState::Manual { .. })
        ));
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            Some(original)
        );
    }

    #[test]
    fn startup_reconciliation_event_failure_rolls_back_pin_and_receipt() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let chosen = ProviderConversationId::new();
        let previous = store
            .get_worker_profile(worker.id)
            .unwrap()
            .provider_conversation_id;
        store.connection().unwrap().execute_batch("CREATE TRIGGER reject_recovery_event BEFORE INSERT ON control_room_events BEGIN SELECT RAISE(ABORT, 'test'); END;").unwrap();
        assert!(
            store
                .reconcile_provider_start(
                    session,
                    attempt(),
                    ProviderSessionStartKind::Resumed,
                    chosen
                )
                .is_err()
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            previous
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_recovery_event;")
            .unwrap();
        assert!(
            store
                .reconcile_provider_start(
                    session,
                    attempt(),
                    ProviderSessionStartKind::Resumed,
                    chosen
                )
                .unwrap()
                .is_some()
        );
    }
}
