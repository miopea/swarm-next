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

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    fn attempt() -> ConversationRecoveryAttempt {
        let ConversationRecoveryState::Attempt { attempt } =
            ConversationRecovery::new(None, true).state()
        else {
            panic!("attempt");
        };
        attempt
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
            .execute_batch("DROP TABLE worker_startup_context; PRAGMA user_version = 126;")
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
