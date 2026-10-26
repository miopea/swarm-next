//! The conversations each worker's own Claude reported to Swarm (ADR 0124).
//!
//! A workspace's Claude project directory is not the worker's alone. Any Claude
//! started in the same directory writes its transcript there too, so a worker
//! whose workspace is a shared parent such as `~/projects` sits beside probes,
//! the operator's own terminals and other tools. Only a conversation reported
//! through this worker's own sessions is evidence about this worker.

use std::collections::{HashMap, HashSet};
use std::str::FromStr as _;

use rusqlite::{OptionalExtension as _, params};
use swarm_domain::{ProviderConversationId, WorkerId, WorkerSessionId};

use crate::{TaskStore, TaskStoreError};

/// How many reported conversations are kept per worker, newest first.
const PER_WORKER: i64 = 32;

pub(crate) fn migrate_reported_conversations(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_reported_conversations (
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
             conversation TEXT NOT NULL CHECK (length(conversation) = 36),
             first_reported_at INTEGER NOT NULL,
             PRIMARY KEY (worker_id, conversation)
         ) WITHOUT ROWID;",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

impl TaskStore {
    /// Records that a live worker session's Claude reported this conversation.
    /// Returns whether it was new for that worker.
    ///
    /// Called on every supervisor pass, so it reads before it writes: a
    /// conversation already on record costs one query and no write.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn record_reported_conversation(
        &self,
        session: WorkerSessionId,
        conversation: ProviderConversationId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let conversation = conversation.to_string();
        let worker: Option<(String, bool)> = connection
            .query_row(
                "SELECT session.worker_id, EXISTS(
                     SELECT 1 FROM worker_reported_conversations reported
                     WHERE reported.worker_id = session.worker_id
                       AND reported.conversation = ?2)
                 FROM worker_sessions session
                 JOIN worker_profiles worker ON worker.id = session.worker_id
                 WHERE session.session_id = ?1 AND session.ended_at IS NULL
                   AND worker.archived_at IS NULL AND worker.provider = 'claude_code'",
                params![session.to_string(), conversation],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((worker, false)) = worker else {
            return Ok(false);
        };
        let transaction = connection.transaction()?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO worker_reported_conversations
                 (worker_id, conversation, first_reported_at)
             VALUES (?1, ?2, unixepoch())",
            params![worker, conversation],
        )?;
        transaction.execute(
            "DELETE FROM worker_reported_conversations
             WHERE worker_id = ?1 AND conversation NOT IN (
                 SELECT conversation FROM worker_reported_conversations
                 WHERE worker_id = ?1
                 ORDER BY first_reported_at DESC, conversation DESC
                 LIMIT ?2)",
            params![worker, PER_WORKER],
        )?;
        transaction.commit()?;
        Ok(inserted == 1)
    }

    /// Every worker's reported conversations, as transcript ids.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn reported_conversations(
        &self,
    ) -> Result<HashMap<WorkerId, HashSet<String>>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT worker_id, conversation FROM worker_reported_conversations")?;
        let mut reported: HashMap<WorkerId, HashSet<String>> = HashMap::new();
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (worker, conversation) = row?;
            if let Ok(worker) = WorkerId::from_str(&worker) {
                reported.entry(worker).or_default().insert(conversation);
            }
        }
        Ok(reported)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ProviderConversationId, ProviderKind, WorkerId, WorkerSessionId};

    use crate::TaskStore;

    fn store_with_live_worker() -> (tempfile::TempDir, TaskStore, WorkerId, WorkerSessionId) {
        let root = tempfile::tempdir().unwrap();
        let store = TaskStore::open(root.path().join("swarm.sqlite")).unwrap();
        let worker = store
            .create_worker("Scout", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        (root, store, worker.id, session)
    }

    #[test]
    fn a_conversation_is_recorded_once_and_kept_per_worker() {
        let (_root, store, worker, session) = store_with_live_worker();
        let conversation = ProviderConversationId::new();

        assert!(
            store
                .record_reported_conversation(session, conversation)
                .unwrap()
        );
        assert!(
            !store
                .record_reported_conversation(session, conversation)
                .unwrap()
        );

        let reported = store.reported_conversations().unwrap();
        assert_eq!(
            reported
                .get(&worker)
                .map(|ids| ids.contains(&conversation.to_string())),
            Some(true)
        );
    }

    #[test]
    fn a_session_that_has_ended_reports_nothing() {
        let (_root, store, _worker, session) = store_with_live_worker();
        store.release_worker_session(session).unwrap();

        assert!(
            !store
                .record_reported_conversation(session, ProviderConversationId::new())
                .unwrap()
        );
        assert!(store.reported_conversations().unwrap().is_empty());
    }

    #[test]
    fn only_the_newest_are_kept() {
        let (_root, store, worker, session) = store_with_live_worker();
        for _ in 0..super::PER_WORKER + 5 {
            store
                .record_reported_conversation(session, ProviderConversationId::new())
                .unwrap();
        }

        let kept = store.reported_conversations().unwrap()[&worker].len();
        assert_eq!(i64::try_from(kept).unwrap(), super::PER_WORKER);
    }
}
