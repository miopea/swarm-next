//! When Swarm moved a worker onto a new conversation of its own accord (ADR 0160).
//!
//! A Claude conversation file only ever grows. Compaction appends a summary and
//! Claude loads only what follows it, but every older line stays in the file,
//! and Claude's own cleanup removes only files left untouched for its cleanup
//! period. A conversation that is resumed at every start is never left
//! untouched, so it is never removed. Queen's had reached 4.1 GB.
//!
//! A rotation moves the pin to a new conversation at a start, so the old file
//! stops being written and ages out. This module records when each rotation
//! happened. The policy that decides when one is due lives in the application
//! layer; this only keeps the facts it reads.

use rusqlite::{OptionalExtension as _, params};
use swarm_domain::{ControlRoomEventKind, ProviderConversationId, WorkerId};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

/// How many rotations are kept per worker, newest first. Only the newest is
/// read; the rest are the trail an operator can check a rotation against.
const PER_WORKER: i64 = 16;

/// One move of a worker's pin onto a new conversation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationRotation {
    /// The conversation the worker was moved onto.
    pub conversation: ProviderConversationId,
    /// The conversation it replaced, which is no longer resumed.
    pub previous: ProviderConversationId,
    /// Unix seconds when the pin moved.
    pub rotated_at: i64,
}

/// Something that says Queen's conversation should not rotate at this start,
/// though it is due (ADR 0160). Queen asked for both, as long as they were
/// cheap to read; each is one row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RotationHold {
    /// An automation run reached Queen and has not finished.
    AutomationRunLive,
    /// Queen's automation is paused while a Steward takeover is active.
    StewardTakeover,
}

pub(crate) fn migrate_conversation_rotations(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_conversation_rotations (
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
             conversation TEXT NOT NULL CHECK (length(conversation) = 36),
             previous TEXT NOT NULL CHECK (length(previous) = 36),
             rotated_at INTEGER NOT NULL,
             PRIMARY KEY (worker_id, conversation)
         ) WITHOUT ROWID;",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

impl TaskStore {
    /// When the worker's CURRENT pinned conversation began, if a rotation
    /// started it.
    ///
    /// `None` means Swarm did not start the pinned conversation by rotating:
    /// it predates rotations, was chosen by the operator, or was reported by
    /// Claude after a clear or fork. Its age is unknown, not young.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn pinned_conversation_rotated_at(
        &self,
        worker_id: WorkerId,
    ) -> Result<Option<i64>, TaskStoreError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT rotation.rotated_at
                 FROM worker_conversation_rotations rotation
                 JOIN worker_profiles worker ON worker.id = rotation.worker_id
                 WHERE rotation.worker_id = ?1
                   AND rotation.conversation = worker.provider_conversation_id",
                [worker_id.to_string()],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Moves a stopped worker's pin from `previous` to `conversation`, and
    /// records the rotation in the same transaction.
    ///
    /// Returns `false` and changes nothing when the pin is no longer
    /// `previous` or the worker has a live session. A start that lost such a
    /// race must not launch on a conversation it did not pin.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn rotate_conversation(
        &self,
        worker_id: WorkerId,
        previous: ProviderConversationId,
        conversation: ProviderConversationId,
        rotated_at: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let worker = worker_id.to_string();
        let moved = transaction.execute(
            "UPDATE worker_profiles
             SET provider_conversation_id = ?2, updated_at = unixepoch()
             WHERE id = ?1 AND provider_conversation_id = ?3 AND archived_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions
                   WHERE worker_id = worker_profiles.id AND ended_at IS NULL)",
            params![worker, conversation.to_string(), previous.to_string()],
        )?;
        if moved == 0 {
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO worker_conversation_rotations
                 (worker_id, conversation, previous, rotated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                worker,
                conversation.to_string(),
                previous.to_string(),
                rotated_at
            ],
        )?;
        transaction.execute(
            "DELETE FROM worker_conversation_rotations
             WHERE worker_id = ?1 AND conversation NOT IN (
                 SELECT conversation FROM worker_conversation_rotations
                 WHERE worker_id = ?1
                 ORDER BY rotated_at DESC, conversation DESC
                 LIMIT ?2)",
            params![worker, PER_WORKER],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(true)
    }

    /// What holds Queen's rotation at this start, if anything does.
    ///
    /// Read-only. A start must not move automation state on the way past, so
    /// this reads the rows `queen_automation_status` reads, without running
    /// its expiry and recovery steps.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn queen_rotation_hold(&self, now: i64) -> Result<Option<RotationHold>, TaskStoreError> {
        let connection = self.connection()?;
        let run_live = connection
            .query_row(
                "SELECT state IN ('delivering', 'running') FROM queen_automation WHERE id = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false);
        if run_live {
            return Ok(Some(RotationHold::AutomationRunLive));
        }
        let takeover: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_federation_steward_takeover_leases
                           WHERE state = 'active' AND expires_at > ?1)",
            [now],
            |row| row.get(0),
        )?;
        Ok(takeover.then_some(RotationHold::StewardTakeover))
    }

    /// The worker's rotations, newest first.
    ///
    /// # Errors
    /// Returns persistence failures, or an integrity failure for a stored id
    /// that is not a conversation id.
    pub fn conversation_rotations(
        &self,
        worker_id: WorkerId,
    ) -> Result<Vec<ConversationRotation>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT conversation, previous, rotated_at FROM worker_conversation_rotations
             WHERE worker_id = ?1 ORDER BY rotated_at DESC, conversation DESC",
        )?;
        let rows = statement.query_map([worker_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (conversation, previous, rotated_at) = row?;
            let parse = |id: String| {
                id.parse::<ProviderConversationId>().map_err(|_| {
                    TaskStoreError::IntegrityFailure("invalid rotated conversation id".into())
                })
            };
            Ok(ConversationRotation {
                conversation: parse(conversation)?,
                previous: parse(previous)?,
                rotated_at,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ProviderConversationId, ProviderKind, WorkerId, WorkerSessionId};

    use crate::TaskStore;

    /// A Claude worker that has run once and stopped, pinned to a conversation.
    fn stopped_worker_with_history() -> (
        tempfile::TempDir,
        TaskStore,
        WorkerId,
        ProviderConversationId,
    ) {
        let root = tempfile::tempdir().unwrap();
        let store = TaskStore::open(root.path().join("swarm.sqlite")).unwrap();
        let worker = store
            .create_worker("Queen", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        // A new Claude worker is given its conversation id when it is created.
        let pinned = worker.provider_conversation_id.unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store.release_worker_session(session).unwrap();
        (root, store, worker.id, pinned)
    }

    fn pin(store: &TaskStore, worker: WorkerId) -> Option<ProviderConversationId> {
        store
            .get_worker_profile(worker)
            .unwrap()
            .provider_conversation_id
    }

    #[test]
    fn a_rotation_moves_the_pin_and_dates_the_new_conversation() {
        let (_root, store, worker, previous) = stopped_worker_with_history();
        assert_eq!(store.pinned_conversation_rotated_at(worker).unwrap(), None);

        let next = ProviderConversationId::new();
        assert!(
            store
                .rotate_conversation(worker, previous, next, 1_000)
                .unwrap()
        );

        assert_eq!(pin(&store, worker), Some(next));
        assert_eq!(
            store.pinned_conversation_rotated_at(worker).unwrap(),
            Some(1_000)
        );
        assert_eq!(
            store.conversation_rotations(worker).unwrap(),
            vec![super::ConversationRotation {
                conversation: next,
                previous,
                rotated_at: 1_000,
            }]
        );
    }

    /// A pin that moved by any other route has no rotation date, so its age is
    /// unknown rather than inherited from the rotation it replaced.
    #[test]
    fn a_pin_moved_elsewhere_has_no_rotation_date() {
        let (_root, store, worker, previous) = stopped_worker_with_history();
        let next = ProviderConversationId::new();
        store
            .rotate_conversation(worker, previous, next, 1_000)
            .unwrap();

        store
            .repoint_provider_conversation_fenced(worker, &ProviderConversationId::new(), None)
            .unwrap();

        assert_eq!(store.pinned_conversation_rotated_at(worker).unwrap(), None);
    }

    #[test]
    fn a_rotation_refuses_a_pin_that_already_moved() {
        let (_root, store, worker, _pinned) = stopped_worker_with_history();
        let stale = ProviderConversationId::new();

        assert!(
            !store
                .rotate_conversation(worker, stale, ProviderConversationId::new(), 1_000)
                .unwrap()
        );

        assert!(store.conversation_rotations(worker).unwrap().is_empty());
    }

    /// The running process keeps writing wherever it already is, so moving the
    /// pin under it would record a rotation that did not happen.
    #[test]
    fn a_rotation_refuses_a_running_worker() {
        let (_root, store, worker, previous) = stopped_worker_with_history();
        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();

        assert!(
            !store
                .rotate_conversation(worker, previous, ProviderConversationId::new(), 1_000)
                .unwrap()
        );

        assert_eq!(pin(&store, worker), Some(previous));
        assert!(store.conversation_rotations(worker).unwrap().is_empty());
    }

    fn set_automation_state(store: &TaskStore, state: &str) {
        let connection = store.connection().unwrap();
        connection
            .execute("INSERT OR IGNORE INTO queen_automation (id) VALUES (1)", [])
            .unwrap();
        connection
            .execute(
                "UPDATE queen_automation SET state = ?1 WHERE id = 1",
                [state],
            )
            .unwrap();
    }

    fn takeover(store: &TaskStore, state: &str, expires_at: i64) {
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO local_federation_steward_takeover_leases
                     (lease_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
                      stewardship_id, reason, state, revision, requested_at, expires_at, synced_at)
                 VALUES (?1, 'apiary', 'source', 'target', 'operator', 'stewardship', 'test',
                         ?2, 1, 0, ?3, 0)",
                rusqlite::params![uuid_like(), state, expires_at],
            )
            .unwrap();
    }

    fn uuid_like() -> String {
        ProviderConversationId::new().to_string()
    }

    #[test]
    fn a_quiet_hive_holds_nothing() {
        let (_root, store, _worker, _pinned) = stopped_worker_with_history();
        assert_eq!(store.queen_rotation_hold(1_000).unwrap(), None);
        set_automation_state(&store, "idle");
        assert_eq!(store.queen_rotation_hold(1_000).unwrap(), None);
    }

    /// A run that reached Queen and has not finished holds the rotation. One
    /// that is waiting to reach her, or is over, does not: holding for those
    /// could put the rotation off indefinitely on a busy board.
    #[test]
    fn only_a_run_that_reached_queen_holds_the_rotation() {
        let (_root, store, _worker, _pinned) = stopped_worker_with_history();
        for (state, held) in [
            ("delivering", true),
            ("running", true),
            ("queued", false),
            ("completed", false),
            ("uncertain", false),
            ("idle", false),
        ] {
            set_automation_state(&store, state);
            assert_eq!(
                store.queen_rotation_hold(1_000).unwrap(),
                held.then_some(super::RotationHold::AutomationRunLive),
                "{state}"
            );
        }
    }

    #[test]
    fn an_active_steward_takeover_holds_the_rotation_until_it_expires() {
        let (_root, store, _worker, _pinned) = stopped_worker_with_history();
        takeover(&store, "released", 5_000);
        assert_eq!(store.queen_rotation_hold(1_000).unwrap(), None);

        takeover(&store, "active", 2_000);
        assert_eq!(
            store.queen_rotation_hold(1_000).unwrap(),
            Some(super::RotationHold::StewardTakeover)
        );
        assert_eq!(store.queen_rotation_hold(2_000).unwrap(), None);
    }

    #[test]
    fn rotations_are_kept_to_a_bounded_trail() {
        let (_root, store, worker, mut previous) = stopped_worker_with_history();
        let total = super::PER_WORKER + 3;
        for at in 0..total {
            let next = ProviderConversationId::new();
            assert!(
                store
                    .rotate_conversation(worker, previous, next, at)
                    .unwrap()
            );
            previous = next;
        }

        let kept = store.conversation_rotations(worker).unwrap();
        assert_eq!(kept.len(), usize::try_from(super::PER_WORKER).unwrap());
        assert_eq!(kept[0].conversation, previous);
        assert_eq!(kept[0].rotated_at, total - 1);
        assert_eq!(kept.last().unwrap().rotated_at, 3);
    }
}
