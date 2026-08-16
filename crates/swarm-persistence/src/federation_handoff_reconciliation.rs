use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    FederationClaimHandoff, FederationClaimHandoffId, FederationClaimHandoffState,
    FederationMembershipReceipt, JiraProjectBindingId,
};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

const MAX_PENDING_FEDERATION_HANDOFFS: i64 = 100;
pub const MAX_FEDERATION_HANDOFF_BATCH: usize = 16;
const MAX_ERROR_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FederationHandoffIntentPhase {
    Accepted,
    JiraAssigned,
    KeeperConfirmed,
    Complete,
    Attention,
}

impl FederationHandoffIntentPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::JiraAssigned => "jira_assigned",
            Self::KeeperConfirmed => "keeper_confirmed",
            Self::Complete => "complete",
            Self::Attention => "attention",
        }
    }
}

impl FromStr for FederationHandoffIntentPhase {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "jira_assigned" => Ok(Self::JiraAssigned),
            "keeper_confirmed" => Ok(Self::KeeperConfirmed),
            "complete" => Ok(Self::Complete),
            "attention" => Ok(Self::Attention),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FederationHandoffIntent {
    pub handoff: FederationClaimHandoff,
    pub binding_id: JiraProjectBindingId,
    pub phase: FederationHandoffIntentPhase,
    pub attempts: u32,
    pub available_at: i64,
    pub last_error: Option<String>,
}

impl TaskStore {
    /// Journals a Keeper-accepted handoff before any local Jira mutation.
    /// Exact retries return the same operation.
    ///
    /// # Errors
    /// Rejects foreign or malformed handoffs, unready project bindings, full
    /// queues, identity drift, and unavailable persistence.
    pub fn journal_accepted_federation_handoff(
        &self,
        handoff: &FederationClaimHandoff,
        now: i64,
    ) -> Result<FederationHandoffIntent, TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 || handoff.state != FederationClaimHandoffState::Accepted {
            return Err(TaskStoreError::InvalidFederationHandoff);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let receipt_json = transaction.query_row(
            "SELECT receipt_json FROM local_federation_membership
             WHERE singleton = 1 AND state = 'active'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let receipt: FederationMembershipReceipt = serde_json::from_str(&receipt_json)
            .map_err(|_| TaskStoreError::InvalidFederationHandoff)?;
        if handoff.apiary_id != receipt.payload.apiary_id
            || handoff.target_node_id != receipt.payload.member_node_id
            || handoff.target_hive_id != identity.hive.id
            || handoff.target_operator_id != identity.operator.id
        {
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        let binding_id = transaction
            .query_row(
                "SELECT id FROM jira_project_bindings
                 WHERE project_id = ?1 AND scope = 'apiary' AND apiary_id = ?2
                   AND access_verified = 1 AND workflow_mapped = 1",
                params![handoff.project_id, handoff.apiary_id.to_string()],
                |row| parse_domain_id(&row.get::<_, String>(0)?),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationHandoff)?;
        let serialized =
            serde_json::to_string(handoff).map_err(|_| TaskStoreError::InvalidFederationHandoff)?;
        if let Some(existing) = handoff_intent_by_id(&transaction, handoff.id)? {
            if existing.handoff == *handoff && existing.binding_id == binding_id {
                return Ok(existing);
            }
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        let pending = transaction.query_row(
            "SELECT COUNT(*) FROM federation_handoff_intents WHERE phase <> 'complete'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if pending >= MAX_PENDING_FEDERATION_HANDOFFS {
            return Err(TaskStoreError::FederationHandoffConflict);
        }
        transaction.execute(
            "INSERT INTO federation_handoff_intents
                (handoff_id, binding_id, handoff_json, phase, attempts,
                 available_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'accepted', 0, ?4, ?4, ?4)",
            params![
                handoff.id.to_string(),
                binding_id.to_string(),
                serialized,
                now
            ],
        )?;
        let intent = FederationHandoffIntent {
            handoff: handoff.clone(),
            binding_id,
            phase: FederationHandoffIntentPhase::Accepted,
            attempts: 0,
            available_at: now,
            last_error: None,
        };
        transaction.commit()?;
        Ok(intent)
    }

    /// Returns the bounded currently eligible receiving-Hive reconciliation batch.
    ///
    /// # Errors
    /// Rejects invalid time, corrupt rows, or unavailable persistence.
    pub fn pending_federation_handoffs(
        &self,
        now: i64,
    ) -> Result<Vec<FederationHandoffIntent>, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationHandoff);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT handoff_id, binding_id, handoff_json, phase, attempts, available_at, last_error
             FROM federation_handoff_intents
             WHERE phase IN ('accepted','jira_assigned','keeper_confirmed') AND available_at <= ?1
             ORDER BY created_at, handoff_id LIMIT ?2",
        )?;
        statement
            .query_map(
                params![now, MAX_FEDERATION_HANDOFF_BATCH],
                handoff_intent_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Advances one handoff operation through its exact expected phase.
    ///
    /// # Errors
    /// Rejects invalid transitions, time, or unavailable persistence.
    pub fn advance_federation_handoff(
        &self,
        id: FederationClaimHandoffId,
        expected: FederationHandoffIntentPhase,
        next: FederationHandoffIntentPhase,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        if now < 0
            || !matches!(
                (expected, next),
                (
                    FederationHandoffIntentPhase::Accepted,
                    FederationHandoffIntentPhase::JiraAssigned
                ) | (
                    FederationHandoffIntentPhase::JiraAssigned,
                    FederationHandoffIntentPhase::KeeperConfirmed
                ) | (
                    FederationHandoffIntentPhase::KeeperConfirmed,
                    FederationHandoffIntentPhase::Complete
                )
            )
        {
            return Err(TaskStoreError::InvalidFederationHandoff);
        }
        Ok(self.connection()?.execute(
            "UPDATE federation_handoff_intents SET phase = ?2, last_error = NULL,
             available_at = ?4, updated_at = ?4 WHERE handoff_id = ?1 AND phase = ?3",
            params![id.to_string(), next.as_str(), expected.as_str(), now],
        )? == 1)
    }

    /// Schedules a bounded retry for a temporary handoff failure.
    ///
    /// # Errors
    /// Rejects invalid error codes, time, or unavailable persistence.
    pub fn retry_federation_handoff(
        &self,
        id: FederationClaimHandoffId,
        now: i64,
        error: &str,
    ) -> Result<bool, TaskStoreError> {
        let error = bounded_error(error, now)?;
        Ok(self.connection()?.execute(
            "UPDATE federation_handoff_intents SET attempts = attempts + 1,
             available_at = ?2 + MIN(300, 15 * (attempts + 1)), last_error = ?3, updated_at = ?2
             WHERE handoff_id = ?1 AND phase IN ('accepted','jira_assigned','keeper_confirmed')",
            params![id.to_string(), now, error],
        )? == 1)
    }

    /// Stops automatic reconciliation when the operator must resolve ambiguity.
    ///
    /// # Errors
    /// Rejects invalid error codes, time, or unavailable persistence.
    pub fn require_attention_for_federation_handoff(
        &self,
        id: FederationClaimHandoffId,
        now: i64,
        error: &str,
    ) -> Result<bool, TaskStoreError> {
        let error = bounded_error(error, now)?;
        Ok(self.connection()?.execute(
            "UPDATE federation_handoff_intents SET phase = 'attention', last_error = ?3, updated_at = ?2
             WHERE handoff_id = ?1 AND phase IN ('accepted','jira_assigned','keeper_confirmed')",
            params![id.to_string(), now, error],
        )? == 1)
    }
}

fn bounded_error(value: &str, now: i64) -> Result<&str, TaskStoreError> {
    let value = value.trim();
    if now < 0
        || value.is_empty()
        || value.len() > MAX_ERROR_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(TaskStoreError::InvalidFederationHandoff);
    }
    Ok(value)
}

fn handoff_intent_by_id(
    connection: &rusqlite::Connection,
    id: FederationClaimHandoffId,
) -> Result<Option<FederationHandoffIntent>, TaskStoreError> {
    connection
        .query_row(
            "SELECT handoff_id, binding_id, handoff_json, phase, attempts, available_at, last_error
         FROM federation_handoff_intents WHERE handoff_id = ?1",
            [id.to_string()],
            handoff_intent_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn handoff_intent_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FederationHandoffIntent> {
    let id = parse_domain_id::<FederationClaimHandoffId>(&row.get::<_, String>(0)?)?;
    let handoff: FederationClaimHandoff = serde_json::from_str(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    if handoff.id != id {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(FederationHandoffIntent {
        handoff,
        binding_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        phase: row
            .get::<_, String>(3)?
            .parse()
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        attempts: row.get(4)?,
        available_at: row.get(5)?,
        last_error: row.get(6)?,
    })
}

pub(crate) fn migrate_federation_handoff_reconciliation(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS federation_handoff_intents (
             handoff_id TEXT PRIMARY KEY,
             binding_id TEXT NOT NULL REFERENCES jira_project_bindings(id),
             handoff_json TEXT NOT NULL,
             phase TEXT NOT NULL CHECK (phase IN ('accepted','jira_assigned','keeper_confirmed','complete','attention')),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
             available_at INTEGER NOT NULL, last_error TEXT,
             created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS federation_handoff_delivery
             ON federation_handoff_intents(phase, available_at, created_at);
         PRAGMA user_version = 55;"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_v54_adds_receiving_hive_handoff_journal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let identity = store.local_hive_identity().unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE federation_handoff_intents;
                 PRAGMA user_version = 54;",
            )
            .unwrap();
        drop(store);
        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(migrated.local_hive_identity().unwrap(), identity);
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'federation_handoff_intents'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }
}
