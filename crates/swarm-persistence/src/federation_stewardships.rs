use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    FEDERATION_PROTOCOL_VERSION, FEDERATION_STEWARDSHIP_SCHEMA_VERSION,
    FederationMembershipReceipt, FederationStewardshipSnapshot, LocalApiaryContext,
    LocalApiaryRole, StewardCapability,
};

use super::{
    TaskStore, TaskStoreError,
    federation::{authenticate_member_credential, decode_node_credential},
};

const MAX_STEWARD_HIVES: usize = 64;
const MAX_STEWARD_CAPABILITIES: usize = 6;

impl TaskStore {
    /// Returns the authenticated Member operator's current Steward delegation.
    /// No private Hive execution or integration data enters this snapshot.
    ///
    /// # Errors
    /// Rejects invalid member credentials, non-Keepers, invalid time, corrupt
    /// grants, and unavailable persistence.
    pub fn federation_stewardship_snapshot(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationStewardshipSnapshot, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidStewardship);
        }
        let credential = decode_node_credential(node_credential)?;
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let member = authenticate_member_credential(&connection, &identity, &credential, now)?;
        drop(connection);
        let stewardship = self
            .stewardships_for_apiary(member.apiary)?
            .into_iter()
            .find(|scope| scope.steward_operator_id == member.operator);
        Ok(FederationStewardshipSnapshot {
            schema_version: FEDERATION_STEWARDSHIP_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            apiary_id: member.apiary,
            member_node_id: member.node,
            member_operator_id: member.operator,
            stewardship,
            generated_at: now,
        })
    }

    /// Replaces the Member's local Steward authority projection atomically.
    /// A snapshot without a delegation explicitly revokes the prior projection.
    ///
    /// # Errors
    /// Rejects personal/Keeper Hives and foreign, malformed, oversized, or
    /// incompatible snapshots.
    pub fn apply_federation_stewardship_snapshot(
        &self,
        snapshot: &FederationStewardshipSnapshot,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.require_local_federation_member()?;
        validate_snapshot_shape(snapshot, now)?;
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidStewardship);
        };
        if local_role != LocalApiaryRole::Member
            || snapshot.apiary_id != apiary.id
            || snapshot.member_operator_id != identity.operator.id
        {
            return Err(TaskStoreError::InvalidStewardship);
        }
        let connection = self.connection()?;
        let receipt_json = connection.query_row(
            "SELECT receipt_json FROM local_federation_membership WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let receipt: FederationMembershipReceipt =
            serde_json::from_str(&receipt_json).map_err(|_| TaskStoreError::InvalidStewardship)?;
        if receipt.payload.member_node_id != snapshot.member_node_id
            || receipt.payload.member_operator_id != snapshot.member_operator_id
        {
            return Err(TaskStoreError::InvalidStewardship);
        }
        drop(connection);

        let serialized =
            serde_json::to_string(snapshot).map_err(|_| TaskStoreError::InvalidStewardship)?;
        self.connection()?.execute(
            "INSERT INTO local_federation_stewardship
                (singleton, snapshot_json, synced_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
                snapshot_json = excluded.snapshot_json,
                synced_at = excluded.synced_at",
            params![serialized, now],
        )?;
        Ok(())
    }

    /// Returns the last Keeper-confirmed local Steward projection, including an
    /// explicit empty projection after revocation.
    ///
    /// # Errors
    /// Returns an error when the stored snapshot is corrupt or unavailable.
    pub fn local_federation_stewardship_snapshot(
        &self,
    ) -> Result<Option<FederationStewardshipSnapshot>, TaskStoreError> {
        self.connection()?
            .query_row(
                "SELECT snapshot_json FROM local_federation_stewardship WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|serialized| {
                serde_json::from_str(&serialized).map_err(|_| TaskStoreError::InvalidStewardship)
            })
            .transpose()
    }
}

fn validate_snapshot_shape(
    snapshot: &FederationStewardshipSnapshot,
    now: i64,
) -> Result<(), TaskStoreError> {
    if snapshot.schema_version != FEDERATION_STEWARDSHIP_SCHEMA_VERSION
        || snapshot.protocol_version != FEDERATION_PROTOCOL_VERSION
        || snapshot.generated_at < 0
        || snapshot.generated_at > now.saturating_add(300)
    {
        return Err(TaskStoreError::InvalidStewardship);
    }
    let Some(scope) = snapshot.stewardship.as_ref() else {
        return Ok(());
    };
    if scope.apiary_id != snapshot.apiary_id
        || scope.steward_operator_id != snapshot.member_operator_id
        || scope.managed_hive_ids.is_empty()
        || scope.managed_hive_ids.len() > MAX_STEWARD_HIVES
        || scope.capabilities.is_empty()
        || scope.capabilities.len() > MAX_STEWARD_CAPABILITIES
        || !scope.capabilities.contains(&StewardCapability::Observe)
        || has_duplicates(&scope.managed_hive_ids)
        || has_duplicates(&scope.capabilities)
    {
        return Err(TaskStoreError::InvalidStewardship);
    }
    Ok(())
}

fn has_duplicates<T: Eq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .any(|(index, item)| items[index + 1..].contains(item))
}

pub(super) fn migrate_federation_stewardship_projection(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_federation_stewardship (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             snapshot_json TEXT NOT NULL CHECK (length(snapshot_json) <= 65536),
             synced_at INTEGER NOT NULL CHECK (synced_at >= 0)
         );
         PRAGMA user_version = 52;",
    )
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::migrate_federation_stewardship_projection;

    #[test]
    fn migration_creates_one_bounded_local_projection() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 51).unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_federation_stewardship_projection(&transaction).unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            52
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'local_federation_stewardship'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }
}
