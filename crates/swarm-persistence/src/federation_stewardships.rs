use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ApiaryId, FEDERATION_PROTOCOL_VERSION, FEDERATION_STEWARDSHIP_SCHEMA_VERSION,
    FederationMembershipReceipt, FederationStewardHiveObservation, FederationStewardshipSnapshot,
    HiveId, LocalApiaryContext, LocalApiaryRole, StewardCapability,
};

use super::{
    TaskStore, TaskStoreError,
    federation::{authenticate_member_credential, decode_node_credential},
};

const MAX_STEWARD_HIVES: usize = 64;
const MAX_STEWARD_CAPABILITIES: usize = 6;
const MAX_OBSERVED_ITEMS: usize = 1_000_000;

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
        let observations = stewardship
            .as_ref()
            .map(|scope| {
                let connection = self.connection()?;
                scope
                    .managed_hive_ids
                    .iter()
                    .map(|hive_id| {
                        steward_hive_observation(&connection, member.apiary, *hive_id, now)
                    })
                    .collect::<Result<Vec<_>, TaskStoreError>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(FederationStewardshipSnapshot {
            schema_version: FEDERATION_STEWARDSHIP_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            apiary_id: member.apiary,
            member_node_id: member.node,
            member_operator_id: member.operator,
            stewardship,
            observations,
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
        return if snapshot.observations.is_empty() {
            Ok(())
        } else {
            Err(TaskStoreError::InvalidStewardship)
        };
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
    if !snapshot.observations.is_empty()
        && (snapshot.observations.len() != scope.managed_hive_ids.len()
            || has_duplicates(
                &snapshot
                    .observations
                    .iter()
                    .map(|observation| observation.hive_id)
                    .collect::<Vec<_>>(),
            )
            || snapshot.observations.iter().any(|observation| {
                !scope.managed_hive_ids.contains(&observation.hive_id)
                    || observation.ready_swarm_task_count > MAX_OBSERVED_ITEMS
                    || observation.active_swarm_task_count > MAX_OBSERVED_ITEMS
                    || observation.blocked_swarm_task_count > MAX_OBSERVED_ITEMS
                    || observation.review_swarm_task_count > MAX_OBSERVED_ITEMS
                    || observation.active_jira_claim_count > MAX_OBSERVED_ITEMS
                    || observation
                        .last_shared_activity_at
                        .is_some_and(|last| last < 0 || last > now.saturating_add(300))
            }))
    {
        return Err(TaskStoreError::InvalidStewardship);
    }
    Ok(())
}

fn steward_hive_observation(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
    hive_id: HiveId,
    now: i64,
) -> Result<FederationStewardHiveObservation, TaskStoreError> {
    let (ready, active, blocked, review, task_activity): (i64, i64, i64, i64, Option<i64>) =
        connection.query_row(
            "SELECT
                 COALESCE(SUM(CASE WHEN state = 'ready' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN state = 'active' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN state = 'blocked' THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN state = 'review' THEN 1 ELSE 0 END), 0),
                 MAX(updated_at)
             FROM apiary_tasks WHERE apiary_id = ?1 AND home_hive_id = ?2",
            params![apiary_id.to_string(), hive_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
    let (jira_claims, claim_activity): (i64, Option<i64>) = connection.query_row(
        "SELECT COUNT(*), MAX(updated_at)
         FROM apiary_federation_claims
         WHERE apiary_id = ?1 AND home_hive_id = ?2
           AND (state = 'confirmed' OR (state = 'reserved' AND reservation_expires_at > ?3))",
        params![apiary_id.to_string(), hive_id.to_string(), now],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(FederationStewardHiveObservation {
        hive_id,
        ready_swarm_task_count: observed_count(ready)?,
        active_swarm_task_count: observed_count(active)?,
        blocked_swarm_task_count: observed_count(blocked)?,
        review_swarm_task_count: observed_count(review)?,
        active_jira_claim_count: observed_count(jira_claims)?,
        last_shared_activity_at: task_activity.into_iter().chain(claim_activity).max(),
    })
}

fn observed_count(value: i64) -> Result<usize, TaskStoreError> {
    usize::try_from(value).map_err(|_| TaskStoreError::InvalidStewardship)
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
    use swarm_domain::{
        ApiaryId, FederationNodeId, FederationStewardHiveObservation,
        FederationStewardshipSnapshot, HiveId, OperatorId, StewardCapability, Stewardship,
        StewardshipId,
    };

    use super::{migrate_federation_stewardship_projection, validate_snapshot_shape};

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

    #[test]
    fn observation_projection_is_exactly_scoped_and_backward_compatible() {
        let apiary_id = ApiaryId::new();
        let operator_id = OperatorId::new();
        let hive_id = HiveId::new();
        let mut snapshot = FederationStewardshipSnapshot {
            schema_version: 1,
            protocol_version: 1,
            apiary_id,
            member_node_id: FederationNodeId::new(),
            member_operator_id: operator_id,
            stewardship: Some(Stewardship {
                id: StewardshipId::new(),
                apiary_id,
                steward_operator_id: operator_id,
                managed_hive_ids: vec![hive_id],
                capabilities: vec![StewardCapability::Observe],
            }),
            observations: Vec::new(),
            generated_at: 100,
        };
        assert!(validate_snapshot_shape(&snapshot, 100).is_ok());
        let legacy_json = serde_json::json!({
            "schema_version": 1,
            "protocol_version": 1,
            "apiary_id": apiary_id,
            "member_node_id": snapshot.member_node_id,
            "member_operator_id": operator_id,
            "stewardship": snapshot.stewardship.clone(),
            "generated_at": 100,
        });
        let legacy: FederationStewardshipSnapshot = serde_json::from_value(legacy_json).unwrap();
        assert!(legacy.observations.is_empty());

        snapshot.observations = vec![FederationStewardHiveObservation {
            hive_id,
            ready_swarm_task_count: 1,
            active_swarm_task_count: 2,
            blocked_swarm_task_count: 3,
            review_swarm_task_count: 4,
            active_jira_claim_count: 5,
            last_shared_activity_at: Some(99),
        }];
        assert!(validate_snapshot_shape(&snapshot, 100).is_ok());

        snapshot.observations.push(snapshot.observations[0].clone());
        assert!(validate_snapshot_shape(&snapshot, 100).is_err());
        snapshot.observations.truncate(1);
        snapshot.stewardship = None;
        assert!(validate_snapshot_shape(&snapshot, 100).is_err());
    }
}
