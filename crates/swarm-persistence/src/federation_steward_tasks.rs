use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    FederationStewardTaskCommand, FederationStewardTaskCommandId, FederationStewardTaskOutboxEntry,
    FederationStewardTaskOutboxState, FederationStewardTaskOutcome, FederationStewardTaskReceipt,
    HiveId, LocalApiaryContext, LocalApiaryRole, StewardCapability, StewardshipId,
};

use super::{
    TaskStore, TaskStoreError,
    federation::{authenticate_member_credential, decode_node_credential},
    federation_tasks::insert_apiary_task_for_hive,
    parse_domain_id,
};

pub const MAX_FEDERATION_STEWARD_TASK_BATCH: usize = 20;
const MAX_LOCAL_STEWARD_TASK_OUTBOX: usize = 256;
const MAX_STEWARD_TASK_COMMANDS: usize = 10_000;

impl TaskStore {
    /// Applies one retry-stable, credential-bound Steward task command on the
    /// Keeper. Scope and capability are re-evaluated inside the mutation
    /// transaction; an out-of-scope command is durably rejected and audited.
    ///
    /// # Errors
    /// Rejects invalid credentials or commands, foreign Apiary identity,
    /// conflicting retries, exhausted audit bounds, and persistence failures.
    #[allow(clippy::too_many_lines)]
    pub fn apply_federation_steward_task_command(
        &self,
        node_credential: &str,
        command: &FederationStewardTaskCommand,
        now: i64,
    ) -> Result<FederationStewardTaskReceipt, TaskStoreError> {
        validate_command(command, now)?;
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        if command.apiary_id != member.apiary {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }
        let command_json = serde_json::to_string(command)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTask)?;
        if let Some((node_id, prior_command, receipt_json)) = transaction
            .query_row(
                "SELECT member_node_id, command_json, receipt_json
                 FROM apiary_steward_task_commands WHERE command_id = ?1",
                [command.id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
        {
            if node_id != member.node.to_string() || prior_command != command_json {
                return Err(TaskStoreError::InvalidFederationStewardTask);
            }
            return serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationStewardTask);
        }
        let command_count = transaction.query_row(
            "SELECT COUNT(*) FROM apiary_steward_task_commands WHERE apiary_id = ?1",
            [member.apiary.to_string()],
            |row| row.get::<_, usize>(0),
        )?;
        if command_count >= MAX_STEWARD_TASK_COMMANDS {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }

        let stewardship_id = transaction
            .query_row(
                "SELECT stewardship.id
                 FROM stewardships stewardship
                 JOIN stewardship_hive_grants hive
                   ON hive.stewardship_id = stewardship.id AND hive.hive_id = ?3
                 JOIN stewardship_capability_grants capability
                   ON capability.stewardship_id = stewardship.id AND capability.capability = 'assign'
                 WHERE stewardship.apiary_id = ?1
                   AND stewardship.steward_operator_id = ?2
                   AND stewardship.revoked_at IS NULL
                 ORDER BY stewardship.created_at DESC LIMIT 1",
                params![
                    member.apiary.to_string(),
                    member.operator.to_string(),
                    command.target_hive_id.to_string(),
                ],
                |row| parse_domain_id::<StewardshipId>(&row.get::<_, String>(0)?),
            )
            .optional()?;
        let target_active = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM apiary_federation_memberships
                 WHERE apiary_id = ?1 AND member_hive_id = ?2 AND state = 'active'
             )",
            params![
                member.apiary.to_string(),
                command.target_hive_id.to_string()
            ],
            |row| row.get::<_, bool>(0),
        )?;
        let (outcome, task) = if stewardship_id.is_some() && target_active {
            let task = insert_apiary_task_for_hive(
                &transaction,
                member.apiary,
                command.title.trim(),
                command.description.trim(),
                command.priority,
                Some(command.target_hive_id),
                now,
            )?;
            (FederationStewardTaskOutcome::Applied, Some(task))
        } else {
            (FederationStewardTaskOutcome::Rejected, None)
        };
        let receipt = FederationStewardTaskReceipt {
            command_id: command.id,
            outcome,
            stewardship_id,
            task,
            processed_at: now,
        };
        let receipt_json = serde_json::to_string(&receipt)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTask)?;
        transaction.execute(
            "INSERT INTO apiary_steward_task_commands
                (command_id, apiary_id, member_node_id, member_hive_id,
                 member_operator_id, target_hive_id, stewardship_id, command_json,
                 outcome, task_id, receipt_json, processed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                command.id.to_string(),
                member.apiary.to_string(),
                member.node.to_string(),
                member.hive.to_string(),
                member.operator.to_string(),
                command.target_hive_id.to_string(),
                stewardship_id.map(|id| id.to_string()),
                command_json,
                outcome.to_string(),
                receipt.task.as_ref().map(|task| task.id.to_string()),
                receipt_json,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    /// Durably queues one Steward task before network I/O. Local projection is
    /// only an early fail-closed check; Keeper re-authorizes every delivery.
    ///
    /// # Errors
    /// Rejects a non-Member Hive, invalid content, stale or insufficient local
    /// authority, a full bounded outbox, and persistence failures.
    pub fn queue_federation_steward_task(
        &self,
        target_hive_id: HiveId,
        title: &str,
        description: &str,
        priority: swarm_domain::TaskPriority,
        now: i64,
    ) -> Result<FederationStewardTaskOutboxEntry, TaskStoreError> {
        self.require_local_federation_member()?;
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        };
        if local_role != LocalApiaryRole::Member {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }
        let command = FederationStewardTaskCommand {
            id: FederationStewardTaskCommandId::new(),
            apiary_id: apiary.id,
            target_hive_id,
            title: title.trim().to_owned(),
            description: description.trim().to_owned(),
            priority,
            created_at: now,
        };
        validate_command(&command, now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let snapshot_json = transaction
            .query_row(
                "SELECT snapshot_json FROM local_federation_stewardship WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationStewardTask)?;
        let snapshot: swarm_domain::FederationStewardshipSnapshot =
            serde_json::from_str(&snapshot_json)
                .map_err(|_| TaskStoreError::InvalidFederationStewardTask)?;
        let allowed = snapshot.apiary_id == apiary.id
            && snapshot.member_operator_id == identity.operator.id
            && snapshot
                .stewardship
                .as_ref()
                .is_some_and(|scope| scope.allows(target_hive_id, StewardCapability::Assign));
        if !allowed {
            return Err(TaskStoreError::StewardActionDenied);
        }
        let queued = transaction.query_row(
            "SELECT COUNT(*) FROM local_federation_steward_task_commands WHERE state = 'queued'",
            [],
            |row| row.get::<_, usize>(0),
        )?;
        if queued >= MAX_LOCAL_STEWARD_TASK_OUTBOX {
            return Err(TaskStoreError::FederationStewardTaskQueueFull);
        }
        let command_json = serde_json::to_string(&command)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTask)?;
        transaction.execute(
            "INSERT INTO local_federation_steward_task_commands
                (command_id, apiary_id, target_hive_id, command_json, state,
                 attempt_count, last_attempt_at, receipt_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'queued', 0, NULL, NULL, ?5, ?5)",
            params![
                command.id.to_string(),
                command.apiary_id.to_string(),
                command.target_hive_id.to_string(),
                command_json,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(FederationStewardTaskOutboxEntry {
            command,
            state: FederationStewardTaskOutboxState::Queued,
            attempt_count: 0,
            last_attempt_at: None,
            receipt: None,
        })
    }

    /// Returns the oldest bounded batch of commands waiting for Keeper.
    ///
    /// # Errors
    /// Rejects an invalid batch size and returns persistence failures.
    pub fn pending_federation_steward_tasks(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardTaskOutboxEntry>, TaskStoreError> {
        if limit == 0 || limit > MAX_FEDERATION_STEWARD_TASK_BATCH {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_federation_steward_task_commands WHERE state = 'queued'
             ORDER BY created_at, command_id LIMIT ?1",
        )?;
        let limit =
            i64::try_from(limit).map_err(|_| TaskStoreError::InvalidFederationStewardTask)?;
        statement
            .query_map([limit], outbox_entry_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Records one delivery attempt without removing the durable command.
    ///
    /// # Errors
    /// Rejects invalid time, missing or non-queued commands, non-Member Hives,
    /// and persistence failures.
    pub fn record_federation_steward_task_attempt(
        &self,
        command_id: FederationStewardTaskCommandId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }
        let changed = self.connection()?.execute(
            "UPDATE local_federation_steward_task_commands
             SET attempt_count = attempt_count + 1, last_attempt_at = ?1, updated_at = ?1
             WHERE command_id = ?2 AND state = 'queued'",
            params![now, command_id.to_string()],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }
        Ok(())
    }

    /// Applies one exact Keeper receipt and closes the matching local command.
    ///
    /// # Errors
    /// Rejects invalid or conflicting receipts, unknown commands, non-Member
    /// Hives, invalid time, and persistence failures.
    pub fn apply_federation_steward_task_receipt(
        &self,
        receipt: &FederationStewardTaskReceipt,
        now: i64,
    ) -> Result<FederationStewardTaskOutboxEntry, TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 || receipt.processed_at < 0 {
            return Err(TaskStoreError::InvalidFederationStewardTask);
        }
        let receipt_json = serde_json::to_string(receipt)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTask)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let prior = transaction
            .query_row(
                "SELECT receipt_json FROM local_federation_steward_task_commands WHERE command_id = ?1",
                [receipt.command_id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationStewardTask)?;
        if let Some(prior) = prior {
            if prior != receipt_json {
                return Err(TaskStoreError::InvalidFederationStewardTask);
            }
        } else {
            let state = match receipt.outcome {
                FederationStewardTaskOutcome::Applied => FederationStewardTaskOutboxState::Applied,
                FederationStewardTaskOutcome::Rejected => {
                    FederationStewardTaskOutboxState::Rejected
                }
            };
            transaction.execute(
                "UPDATE local_federation_steward_task_commands
                 SET state = ?1, receipt_json = ?2, updated_at = ?3
                 WHERE command_id = ?4 AND state = 'queued'",
                params![
                    state.to_string(),
                    receipt_json,
                    now,
                    receipt.command_id.to_string()
                ],
            )?;
        }
        let entry = transaction.query_row(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_federation_steward_task_commands WHERE command_id = ?1",
            [receipt.command_id.to_string()],
            outbox_entry_from_row,
        )?;
        transaction.commit()?;
        Ok(entry)
    }

    /// Lists recent local Steward command delivery evidence for presentation.
    ///
    /// # Errors
    /// Returns persistence or incompatible stored-record failures.
    pub fn list_federation_steward_task_outbox(
        &self,
    ) -> Result<Vec<FederationStewardTaskOutboxEntry>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_federation_steward_task_commands
             ORDER BY CASE state WHEN 'queued' THEN 0 WHEN 'rejected' THEN 1 ELSE 2 END,
                      updated_at DESC, command_id DESC LIMIT 100",
        )?;
        statement
            .query_map([], outbox_entry_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn validate_command(
    command: &FederationStewardTaskCommand,
    now: i64,
) -> Result<(), TaskStoreError> {
    if now < 0
        || command.created_at < 0
        || command.created_at > now.saturating_add(300)
        || command.title.trim().is_empty()
        || command.title.len() > super::MAX_TASK_TITLE_BYTES
        || command.description.len() > super::MAX_TASK_DESCRIPTION_BYTES
        || command.title.chars().any(char::is_control)
    {
        return Err(TaskStoreError::InvalidFederationStewardTask);
    }
    Ok(())
}

fn outbox_entry_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationStewardTaskOutboxEntry> {
    let command_json = row.get::<_, String>(0)?;
    let receipt_json = row.get::<_, Option<String>>(4)?;
    let attempts = row.get::<_, i64>(2)?;
    Ok(FederationStewardTaskOutboxEntry {
        command: serde_json::from_str(&command_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        state: FederationStewardTaskOutboxState::from_str(&row.get::<_, String>(1)?)
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        attempt_count: u32::try_from(attempts)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, attempts))?,
        last_attempt_at: row.get(3)?,
        receipt: receipt_json
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
    })
}

pub(super) fn migrate_federation_steward_task_commands(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_steward_task_commands (
             command_id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             member_node_id TEXT NOT NULL,
             member_hive_id TEXT NOT NULL REFERENCES hives(id),
             member_operator_id TEXT NOT NULL REFERENCES operators(id),
             target_hive_id TEXT NOT NULL REFERENCES hives(id),
             stewardship_id TEXT REFERENCES stewardships(id),
             command_json TEXT NOT NULL,
             outcome TEXT NOT NULL CHECK (outcome IN ('applied','rejected')),
             task_id TEXT REFERENCES apiary_tasks(id),
             receipt_json TEXT NOT NULL,
             processed_at INTEGER NOT NULL CHECK (processed_at >= 0),
             CHECK ((outcome = 'applied' AND stewardship_id IS NOT NULL AND task_id IS NOT NULL)
                 OR (outcome = 'rejected' AND task_id IS NULL))
         );
         CREATE INDEX IF NOT EXISTS apiary_steward_task_audit
             ON apiary_steward_task_commands(apiary_id, processed_at DESC);
         CREATE TABLE IF NOT EXISTS local_federation_steward_task_commands (
             command_id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             target_hive_id TEXT NOT NULL,
             command_json TEXT NOT NULL,
             state TEXT NOT NULL CHECK (state IN ('queued','applied','rejected')),
             attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
             last_attempt_at INTEGER CHECK (last_attempt_at >= 0),
             receipt_json TEXT,
             created_at INTEGER NOT NULL CHECK (created_at >= 0),
             updated_at INTEGER NOT NULL CHECK (updated_at >= created_at)
         );
         CREATE INDEX IF NOT EXISTS local_federation_steward_task_queue
             ON local_federation_steward_task_commands(state, created_at, command_id);
         PRAGMA user_version = 58;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        FederationJoinAcceptance, FederationJoinReadiness, JiraConnectionState, SharedWorkBackend,
        TaskPriority,
    };

    fn join_member(keeper: &TaskStore, now: i64) -> (TaskStore, FederationJoinAcceptance) {
        let member = TaskStore::in_memory().expect("member");
        let identity = member.local_hive_identity().expect("identity");
        let card = member
            .issue_hive_connection_card(now + 1, 3_600)
            .expect("card");
        keeper.pin_hive_candidate(&card, now + 1).expect("pin");
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                identity.hive.id,
                "https://keeper.example.test/swarm",
                now + 1,
                3_600,
            )
            .expect("invitation");
        let invitation = member
            .import_apiary_invitation_bundle(&bundle, now + 2)
            .expect("import");
        member
            .accept_federation_join_policy(invitation.invitation_id, 1, now + 3)
            .expect("policy");
        let submission = member
            .prepare_federation_join_submission(
                invitation.invitation_id,
                &FederationJoinReadiness {
                    jira_connection: JiraConnectionState::Ready,
                    projects: Vec::new(),
                    blockers: Vec::new(),
                },
                now + 4,
            )
            .expect("submission");
        let acceptance = keeper
            .consume_federation_join_submission(&submission, now + 5)
            .expect("acceptance");
        member
            .apply_federation_join_acceptance(
                acceptance.receipt.payload.invitation_id,
                &acceptance,
                now + 6,
            )
            .expect("join");
        (member, acceptance)
    }

    #[test]
    fn steward_routes_one_retry_stable_task_to_an_explicitly_managed_hive() {
        let now = 200_000;
        let keeper = TaskStore::in_memory().expect("keeper");
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .expect("apiary");
        let (steward, steward_acceptance) = join_member(&keeper, now + 10);
        let (_target, target_acceptance) = join_member(&keeper, now + 30);
        let steward_identity = steward.local_hive_identity().expect("identity");
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        keeper
            .set_stewardship(
                steward_identity.operator.id,
                &[target_hive_id],
                &[StewardCapability::Observe, StewardCapability::Assign],
                now + 50,
            )
            .expect("stewardship");
        let snapshot = keeper
            .federation_stewardship_snapshot(&steward_acceptance.node_credential, now + 51)
            .expect("snapshot");
        steward
            .apply_federation_stewardship_snapshot(&snapshot, now + 52)
            .expect("projection");

        let queued = steward
            .queue_federation_steward_task(
                target_hive_id,
                "Investigate the shared failure",
                "Return a verified outcome to the Keeper.",
                TaskPriority::High,
                now + 53,
            )
            .expect("queue");
        assert_eq!(queued.state, FederationStewardTaskOutboxState::Queued);
        steward
            .record_federation_steward_task_attempt(queued.command.id, now + 54)
            .expect("attempt");
        let receipt = keeper
            .apply_federation_steward_task_command(
                &steward_acceptance.node_credential,
                &queued.command,
                now + 55,
            )
            .expect("apply");
        assert_eq!(receipt.outcome, FederationStewardTaskOutcome::Applied);
        assert_eq!(
            receipt.task.as_ref().and_then(|task| task.home_hive_id),
            Some(target_hive_id)
        );
        assert_eq!(
            keeper
                .apply_federation_steward_task_command(
                    &steward_acceptance.node_credential,
                    &queued.command,
                    now + 56,
                )
                .expect("exact retry"),
            receipt
        );
        steward
            .apply_federation_steward_task_receipt(&receipt, now + 57)
            .expect("receipt");
        let outbox = steward
            .list_federation_steward_task_outbox()
            .expect("outbox");
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].state, FederationStewardTaskOutboxState::Applied);
        let connection = keeper.connection().expect("connection");
        let task_count = connection
            .query_row(
                "SELECT COUNT(*) FROM apiary_tasks WHERE title = ?1",
                ["Investigate the shared failure"],
                |row| row.get::<_, usize>(0),
            )
            .expect("task count");
        let audit_count = connection
            .query_row(
                "SELECT COUNT(*) FROM apiary_steward_task_commands WHERE command_id = ?1",
                [queued.command.id.to_string()],
                |row| row.get::<_, usize>(0),
            )
            .expect("audit count");
        assert_eq!(task_count, 1);
        assert_eq!(audit_count, 1);
    }

    #[test]
    fn keeper_rejects_and_audits_a_command_outside_current_steward_scope() {
        let now = 300_000;
        let keeper = TaskStore::in_memory().expect("keeper");
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .expect("apiary");
        let (steward, acceptance) = join_member(&keeper, now + 10);
        let (_target, target_acceptance) = join_member(&keeper, now + 30);
        let steward_identity = steward.local_hive_identity().expect("identity");
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let stewardship = keeper
            .set_stewardship(
                steward_identity.operator.id,
                &[target_hive_id],
                &[StewardCapability::Observe, StewardCapability::Assign],
                now + 50,
            )
            .expect("stewardship");
        keeper
            .revoke_stewardship(stewardship.id, now + 51)
            .expect("revoke");
        let context = keeper.local_apiary_context().expect("context");
        let LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("keeper must be federated");
        };
        let command = FederationStewardTaskCommand {
            id: FederationStewardTaskCommandId::new(),
            apiary_id: apiary.id,
            target_hive_id,
            title: "Must not be created".to_owned(),
            description: String::new(),
            priority: TaskPriority::Normal,
            created_at: now + 52,
        };
        let receipt = keeper
            .apply_federation_steward_task_command(&acceptance.node_credential, &command, now + 53)
            .expect("durable rejection");
        assert_eq!(receipt.outcome, FederationStewardTaskOutcome::Rejected);
        assert!(receipt.task.is_none());
        let connection = keeper.connection().expect("connection");
        let task_count = connection
            .query_row(
                "SELECT COUNT(*) FROM apiary_tasks WHERE title = ?1",
                ["Must not be created"],
                |row| row.get::<_, usize>(0),
            )
            .expect("task count");
        let outcome = connection
            .query_row(
                "SELECT outcome FROM apiary_steward_task_commands WHERE command_id = ?1",
                [command.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("audit outcome");
        assert_eq!(task_count, 0);
        assert_eq!(outcome, "rejected");
    }
}
