use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, QueenAutonomyLevel, QueenAutonomyPolicy};

use super::{TaskStore, TaskStoreError, insert_control_room_event, presence::local_operator_id};

impl TaskStore {
    /// Returns the durable, deterministic Queen autonomy policy for this Hive.
    ///
    /// # Errors
    /// Returns persistence or integrity failures.
    pub fn queen_autonomy_policy(&self) -> Result<QueenAutonomyPolicy, TaskStoreError> {
        let connection = self.connection()?;
        queen_autonomy_policy_from_connection(&connection)
    }

    /// Replaces all presence tiers atomically and emits one control-room invalidation.
    ///
    /// # Errors
    /// Returns persistence or integrity failures.
    pub fn set_queen_autonomy_policy(
        &self,
        policy: QueenAutonomyPolicy,
        now: i64,
    ) -> Result<QueenAutonomyPolicy, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        let before = queen_autonomy_policy_from_connection(&transaction)?;
        transaction.execute(
            "INSERT INTO queen_autonomy_preferences (
                 operator_id, at_hive, away, night_watch, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(operator_id) DO UPDATE SET
                 at_hive = excluded.at_hive, away = excluded.away,
                 night_watch = excluded.night_watch, updated_at = excluded.updated_at",
            params![
                operator_id.to_string(),
                policy.at_hive.to_string(),
                policy.away.to_string(),
                policy.night_watch.to_string(),
                now,
            ],
        )?;
        if before != policy {
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        transaction.commit()?;
        Ok(policy)
    }
}

pub(super) fn queen_autonomy_policy_from_connection(
    connection: &Connection,
) -> Result<QueenAutonomyPolicy, TaskStoreError> {
    let operator_id = local_operator_id(connection)?;
    let stored = connection
        .query_row(
            "SELECT at_hive, away, night_watch FROM queen_autonomy_preferences
             WHERE operator_id = ?1",
            [operator_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((at_hive, away, night_watch)) = stored else {
        return Ok(QueenAutonomyPolicy::default());
    };
    Ok(QueenAutonomyPolicy {
        at_hive: parse_level(&at_hive)?,
        away: parse_level(&away)?,
        night_watch: parse_level(&night_watch)?,
    })
}

fn parse_level(value: &str) -> Result<QueenAutonomyLevel, TaskStoreError> {
    QueenAutonomyLevel::from_str(value)
        .map_err(|_| TaskStoreError::IntegrityFailure("invalid Queen autonomy level".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queen_policy_defaults_and_persists_as_one_atomic_setting() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(
            store.queen_autonomy_policy().unwrap(),
            QueenAutonomyPolicy::default()
        );
        let policy = QueenAutonomyPolicy {
            at_hive: QueenAutonomyLevel::LocalExecution,
            away: QueenAutonomyLevel::Advisory,
            night_watch: QueenAutonomyLevel::Coordinate,
        };
        assert_eq!(store.set_queen_autonomy_policy(policy, 42).unwrap(), policy);
        assert_eq!(store.queen_autonomy_policy().unwrap(), policy);
    }
}
