use std::str::FromStr;

use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, FederationStewardTakeoverAction, FederationStewardTakeoverCommand,
    FederationStewardTakeoverCommandId, FederationStewardTakeoverInbox,
    FederationStewardTakeoverLease, FederationStewardTakeoverLeaseId,
    FederationStewardTakeoverLocalState, FederationStewardTakeoverOutboxEntry,
    FederationStewardTakeoverOutboxState, FederationStewardTakeoverOutcome,
    FederationStewardTakeoverReceipt, FederationStewardTakeoverState, HiveId, LocalApiaryContext,
    LocalApiaryRole, StewardCapability, StewardshipId,
};

use super::{
    TaskStore, TaskStoreError,
    federation::{MemberCredentialContext, authenticate_member_credential, decode_node_credential},
    insert_control_room_event, parse_domain_id,
};

pub const MAX_FEDERATION_STEWARD_TAKEOVER_BATCH: usize = 20;
pub const STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION: u16 = 1;
// Protocol 8 is reserved for the terminal-host takeover relay commands. Until
// that host protocol ships, no public API may enqueue these commands.
pub const STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION: u16 = 8;
const MAX_LOCAL_TAKEOVER_OUTBOX: usize = 256;
const MAX_KEEPER_TAKEOVER_COMMANDS: usize = 10_000;
const MAX_TAKEOVER_REASON_BYTES: usize = 2_000;
const REQUEST_LIFETIME_SECONDS: i64 = 60;
const ACTIVE_LIFETIME_SECONDS: i64 = 300;

impl TaskStore {
    /// Applies one authenticated, retry-stable takeover transition at Keeper.
    /// Requested leases grant no terminal visibility or input authority.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid credentials, malformed commands, or
    /// persistence failures. Authorization denials are durable receipts.
    pub fn apply_federation_steward_takeover_command(
        &self,
        node_credential: &str,
        command: &FederationStewardTakeoverCommand,
        now: i64,
    ) -> Result<FederationStewardTakeoverReceipt, TaskStoreError> {
        validate_command(command, now)?;
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        if command.apiary_id != member.apiary {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        expire_open_leases(&transaction, member.apiary, now)?;
        let command_json = serde_json::to_string(command)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover)?;
        if let Some((node_id, prior_command, receipt_json)) = transaction
            .query_row(
                "SELECT member_node_id, command_json, receipt_json
                 FROM apiary_steward_takeover_commands WHERE command_id = ?1",
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
                return Err(TaskStoreError::InvalidFederationStewardTakeover);
            }
            return serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover);
        }
        let command_count = transaction.query_row(
            "SELECT COUNT(*) FROM apiary_steward_takeover_commands WHERE apiary_id = ?1",
            [member.apiary.to_string()],
            |row| row.get::<_, usize>(0),
        )?;
        if command_count >= MAX_KEEPER_TAKEOVER_COMMANDS {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        let (outcome, lease) = apply_authenticated_command(&transaction, &member, command, now)?;
        let receipt = FederationStewardTakeoverReceipt {
            command_id: command.id,
            outcome,
            lease,
            processed_at: now,
        };
        let receipt_json = serde_json::to_string(&receipt)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover)?;
        transaction.execute(
            "INSERT INTO apiary_steward_takeover_commands
                (command_id, apiary_id, member_node_id, member_hive_id,
                 member_operator_id, command_json, outcome, lease_id,
                 receipt_json, processed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                command.id.to_string(),
                member.apiary.to_string(),
                member.node.to_string(),
                member.hive.to_string(),
                member.operator.to_string(),
                command_json,
                outcome.to_string(),
                receipt.lease.as_ref().map(|lease| lease.id.to_string()),
                receipt_json,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    /// Returns only takeover leases involving the authenticated Member Hive.
    ///
    /// # Errors
    ///
    /// Returns an error when authentication or storage fails.
    pub fn federation_steward_takeover_inbox(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverInbox, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        expire_open_leases(&transaction, member.apiary, now)?;
        let leases = read_leases(
            &transaction,
            "apiary_steward_takeover_leases",
            "WHERE apiary_id = ?1 AND (source_hive_id = ?2 OR target_hive_id = ?2)
             ORDER BY requested_at DESC, lease_id DESC LIMIT 100",
            params![member.apiary.to_string(), member.hive.to_string()],
        )?;
        transaction.commit()?;
        Ok(FederationStewardTakeoverInbox {
            leases,
            generated_at: now,
        })
    }

    /// Journals a reasoned Steward takeover request before network I/O.
    /// This internal foundation is deliberately not exposed through HTTP until
    /// terminal relay, reclaim, and visible audit are complete.
    ///
    /// # Errors
    ///
    /// Returns an error when the synchronized scope does not authorize the
    /// target, the protocol is unsupported, or the bounded queue is full.
    pub fn queue_federation_steward_takeover(
        &self,
        target_hive_id: HiveId,
        reason: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        self.queue_local_takeover(
            FederationStewardTakeoverAction::Request {
                target_hive_id,
                reason: reason.trim().to_owned(),
                relay_protocol_version: STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION,
                terminal_protocol_version: STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION,
            },
            now,
        )
    }

    /// Journals the target Hive's exact acknowledgement of a requested lease.
    ///
    /// # Errors
    ///
    /// Returns an error unless this Hive is the target and the exact requested
    /// revision exists in its local projection.
    pub fn queue_federation_steward_takeover_acknowledgement(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        self.queue_local_takeover(
            FederationStewardTakeoverAction::Acknowledge {
                lease_id,
                expected_revision,
                relay_protocol_version: STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION,
                terminal_protocol_version: STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION,
            },
            now,
        )
    }

    /// Journals a target-operator reclaim, which wins over remote control.
    ///
    /// # Errors
    ///
    /// Returns an error unless this Hive owns the target Queen lease.
    pub fn queue_federation_steward_takeover_reclaim(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        reason: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        self.queue_local_takeover(
            FederationStewardTakeoverAction::Reclaim {
                lease_id,
                expected_revision,
                reason: reason.trim().to_owned(),
            },
            now,
        )
    }

    /// Journals a source-Steward renewal after authenticated input.
    ///
    /// # Errors
    ///
    /// Returns an error unless this Hive owns the active source lease.
    pub fn queue_federation_steward_takeover_renewal(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        self.queue_local_takeover(
            FederationStewardTakeoverAction::Renew {
                lease_id,
                expected_revision,
            },
            now,
        )
    }

    /// Journals a source-Steward release.
    ///
    /// # Errors
    ///
    /// Returns an error unless this Hive owns the source lease.
    pub fn queue_federation_steward_takeover_release(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        self.queue_local_takeover(
            FederationStewardTakeoverAction::Release {
                lease_id,
                expected_revision,
            },
            now,
        )
    }

    fn queue_local_takeover(
        &self,
        action: FederationStewardTakeoverAction,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        self.require_local_federation_member()?;
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated {
            apiary,
            local_role: LocalApiaryRole::Member,
        } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        };
        let command = FederationStewardTakeoverCommand {
            id: FederationStewardTakeoverCommandId::new(),
            apiary_id: apiary.id,
            action,
            created_at: now,
        };
        validate_command(&command, now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        authorize_local_action(&transaction, &identity, apiary.id, &command.action, now)?;
        let queued = transaction.query_row(
            "SELECT COUNT(*) FROM local_federation_steward_takeover_commands WHERE state = 'queued'",
            [],
            |row| row.get::<_, usize>(0),
        )?;
        if queued >= MAX_LOCAL_TAKEOVER_OUTBOX {
            return Err(TaskStoreError::FederationStewardTakeoverQueueFull);
        }
        let command_json = serde_json::to_string(&command)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover)?;
        transaction.execute(
            "INSERT INTO local_federation_steward_takeover_commands
                (command_id, apiary_id, command_json, state, attempt_count,
                 last_attempt_at, receipt_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'queued', 0, NULL, NULL, ?4, ?4)",
            params![
                command.id.to_string(),
                command.apiary_id.to_string(),
                command_json,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(FederationStewardTakeoverOutboxEntry {
            command,
            state: FederationStewardTakeoverOutboxState::Queued,
            attempt_count: 0,
            last_attempt_at: None,
            receipt: None,
        })
    }

    /// Returns a bounded delivery batch.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit or storage failure.
    pub fn pending_federation_steward_takeovers(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardTakeoverOutboxEntry>, TaskStoreError> {
        if limit == 0 || limit > MAX_FEDERATION_STEWARD_TAKEOVER_BATCH {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        self.read_takeover_outbox(
            "WHERE state = 'queued' ORDER BY created_at, command_id LIMIT ?1",
            Some(limit),
        )
    }

    /// Records a delivery attempt for a queued command.
    ///
    /// # Errors
    ///
    /// Returns an error when the command is no longer queued.
    pub fn record_federation_steward_takeover_attempt(
        &self,
        command_id: FederationStewardTakeoverCommandId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let changed = self.connection()?.execute(
            "UPDATE local_federation_steward_takeover_commands
             SET attempt_count = attempt_count + 1, last_attempt_at = ?1, updated_at = ?1
             WHERE command_id = ?2 AND state = 'queued'",
            params![now, command_id.to_string()],
        )?;
        (changed == 1)
            .then_some(())
            .ok_or(TaskStoreError::InvalidFederationStewardTakeover)
    }

    /// Applies Keeper's durable receipt to the local outbox.
    ///
    /// # Errors
    ///
    /// Returns an error for a mismatched or already-resolved command.
    pub fn apply_federation_steward_takeover_receipt(
        &self,
        receipt: &FederationStewardTakeoverReceipt,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, TaskStoreError> {
        let state = match receipt.outcome {
            FederationStewardTakeoverOutcome::Applied => {
                FederationStewardTakeoverOutboxState::Applied
            }
            FederationStewardTakeoverOutcome::Rejected => {
                FederationStewardTakeoverOutboxState::Rejected
            }
            FederationStewardTakeoverOutcome::Conflict => {
                FederationStewardTakeoverOutboxState::Conflict
            }
        };
        let receipt_json = serde_json::to_string(receipt)
            .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover)?;
        let changed = self.connection()?.execute(
            "UPDATE local_federation_steward_takeover_commands
             SET state = ?1, receipt_json = ?2, updated_at = ?3
             WHERE command_id = ?4 AND state = 'queued'",
            params![
                state.to_string(),
                receipt_json,
                now,
                receipt.command_id.to_string()
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        self.read_takeover_outbox("ORDER BY created_at DESC LIMIT 100", None)?
            .into_iter()
            .find(|entry| entry.command.id == receipt.command_id)
            .ok_or(TaskStoreError::InvalidFederationStewardTakeover)
    }

    /// Atomically replaces the local public lease projection.
    ///
    /// # Errors
    ///
    /// Returns an error for a foreign, oversized, future, or malformed inbox.
    pub fn apply_federation_steward_takeover_inbox(
        &self,
        inbox: &FederationStewardTakeoverInbox,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated {
            apiary,
            local_role: LocalApiaryRole::Member,
        } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        };
        if inbox.generated_at > now.saturating_add(300)
            || inbox.leases.len() > 100
            || inbox.leases.iter().any(|lease| {
                lease.apiary_id != apiary.id
                    || (lease.source_hive_id != identity.hive.id
                        && lease.target_hive_id != identity.hive.id)
                    || !valid_lease(lease, now)
            })
        {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let existing = read_leases(
            &transaction,
            "local_federation_steward_takeover_leases",
            "ORDER BY requested_at DESC, lease_id DESC",
            [],
        )?;
        if existing == inbox.leases {
            return Ok(());
        }
        transaction.execute("DELETE FROM local_federation_steward_takeover_leases", [])?;
        for lease in &inbox.leases {
            insert_local_lease(&transaction, lease, now)?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the local lease projection and durable command outbox.
    ///
    /// # Errors
    ///
    /// Returns an error when stored state is invalid.
    pub fn federation_steward_takeover_local_state(
        &self,
    ) -> Result<FederationStewardTakeoverLocalState, TaskStoreError> {
        let connection = self.connection()?;
        let leases = read_leases(
            &connection,
            "local_federation_steward_takeover_leases",
            "ORDER BY requested_at DESC, lease_id DESC",
            [],
        )?;
        drop(connection);
        Ok(FederationStewardTakeoverLocalState {
            leases,
            outbox: self.read_takeover_outbox("ORDER BY created_at DESC LIMIT 100", None)?,
        })
    }

    fn read_takeover_outbox(
        &self,
        suffix: &str,
        limit: Option<usize>,
    ) -> Result<Vec<FederationStewardTakeoverOutboxEntry>, TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_federation_steward_takeover_commands {suffix}"
        );
        let mut statement = connection.prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| takeover_outbox_from_row(row);
        if let Some(limit) = limit {
            statement
                .query_map(
                    [i64::try_from(limit)
                        .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover)?],
                    map,
                )?
                .collect::<Result<Vec<_>, _>>()
                .map_err(Into::into)
        } else {
            statement
                .query_map([], map)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(Into::into)
        }
    }
}

fn authorize_local_action(
    transaction: &Transaction<'_>,
    identity: &swarm_domain::HiveIdentity,
    apiary_id: swarm_domain::ApiaryId,
    action: &FederationStewardTakeoverAction,
    now: i64,
) -> Result<(), TaskStoreError> {
    match action {
        FederationStewardTakeoverAction::Request { target_hive_id, .. } => {
            let snapshot_json = transaction
                .query_row(
                    "SELECT snapshot_json FROM local_federation_stewardship WHERE singleton = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(TaskStoreError::StewardActionDenied)?;
            let snapshot: swarm_domain::FederationStewardshipSnapshot =
                serde_json::from_str(&snapshot_json)
                    .map_err(|_| TaskStoreError::InvalidFederationStewardTakeover)?;
            if snapshot.apiary_id != apiary_id
                || snapshot.member_operator_id != identity.operator.id
                || !snapshot
                    .stewardship
                    .as_ref()
                    .is_some_and(|scope| scope.allows(*target_hive_id, StewardCapability::Takeover))
            {
                return Err(TaskStoreError::StewardActionDenied);
            }
        }
        FederationStewardTakeoverAction::Acknowledge {
            lease_id,
            expected_revision,
            ..
        } => require_local_lease(
            transaction,
            *lease_id,
            identity.hive.id,
            false,
            *expected_revision,
            FederationStewardTakeoverState::Requested,
        )?,
        FederationStewardTakeoverAction::Reclaim {
            lease_id,
            expected_revision,
            ..
        } => {
            require_local_lease(
                transaction,
                *lease_id,
                identity.hive.id,
                false,
                *expected_revision,
                FederationStewardTakeoverState::Active,
            )?;
            transaction.execute(
                "UPDATE local_federation_steward_takeover_leases
                 SET state = 'reclaimed', revision = revision + 1,
                     ended_at = ?1, synced_at = ?1
                 WHERE lease_id = ?2 AND revision = ?3 AND state = 'active'",
                params![now, lease_id.to_string(), expected_revision],
            )?;
            insert_control_room_event(transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        FederationStewardTakeoverAction::Renew {
            lease_id,
            expected_revision,
        }
        | FederationStewardTakeoverAction::Release {
            lease_id,
            expected_revision,
        } => require_local_lease(
            transaction,
            *lease_id,
            identity.hive.id,
            true,
            *expected_revision,
            FederationStewardTakeoverState::Active,
        )?,
    }
    Ok(())
}

fn apply_authenticated_command(
    transaction: &Transaction<'_>,
    member: &MemberCredentialContext,
    command: &FederationStewardTakeoverCommand,
    now: i64,
) -> Result<
    (
        FederationStewardTakeoverOutcome,
        Option<FederationStewardTakeoverLease>,
    ),
    TaskStoreError,
> {
    match &command.action {
        FederationStewardTakeoverAction::Request {
            target_hive_id,
            reason,
            ..
        } => {
            let Some(stewardship_id) = authorized_stewardship(
                transaction,
                member.apiary,
                member.operator,
                *target_hive_id,
            )?
            else {
                return Ok((FederationStewardTakeoverOutcome::Rejected, None));
            };
            if !active_member_hive(transaction, member.apiary, *target_hive_id)? {
                return Ok((FederationStewardTakeoverOutcome::Rejected, None));
            }
            if open_lease_for_target(transaction, member.apiary, *target_hive_id)?.is_some() {
                return Ok((FederationStewardTakeoverOutcome::Conflict, None));
            }
            let lease = FederationStewardTakeoverLease {
                id: FederationStewardTakeoverLeaseId::new(),
                apiary_id: member.apiary,
                source_hive_id: member.hive,
                target_hive_id: *target_hive_id,
                source_operator_id: member.operator,
                stewardship_id,
                reason: reason.trim().to_owned(),
                state: FederationStewardTakeoverState::Requested,
                revision: 1,
                requested_at: now,
                acknowledged_at: None,
                expires_at: now.saturating_add(REQUEST_LIFETIME_SECONDS),
                ended_at: None,
            };
            insert_keeper_lease(transaction, &lease)?;
            Ok((FederationStewardTakeoverOutcome::Applied, Some(lease)))
        }
        FederationStewardTakeoverAction::Acknowledge {
            lease_id,
            expected_revision,
            ..
        } => transition_lease(
            transaction,
            member,
            *lease_id,
            *expected_revision,
            FederationStewardTakeoverState::Requested,
            FederationStewardTakeoverState::Active,
            now,
            true,
            false,
        ),
        FederationStewardTakeoverAction::Renew {
            lease_id,
            expected_revision,
        } => transition_lease(
            transaction,
            member,
            *lease_id,
            *expected_revision,
            FederationStewardTakeoverState::Active,
            FederationStewardTakeoverState::Active,
            now,
            false,
            true,
        ),
        FederationStewardTakeoverAction::Release {
            lease_id,
            expected_revision,
        } => transition_lease(
            transaction,
            member,
            *lease_id,
            *expected_revision,
            FederationStewardTakeoverState::Active,
            FederationStewardTakeoverState::Released,
            now,
            false,
            true,
        ),
        FederationStewardTakeoverAction::Reclaim {
            lease_id,
            expected_revision,
            ..
        } => transition_lease(
            transaction,
            member,
            *lease_id,
            *expected_revision,
            FederationStewardTakeoverState::Active,
            FederationStewardTakeoverState::Reclaimed,
            now,
            true,
            false,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn transition_lease(
    transaction: &Transaction<'_>,
    member: &MemberCredentialContext,
    lease_id: FederationStewardTakeoverLeaseId,
    expected_revision: u64,
    from: FederationStewardTakeoverState,
    to: FederationStewardTakeoverState,
    now: i64,
    target_action: bool,
    source_action: bool,
) -> Result<
    (
        FederationStewardTakeoverOutcome,
        Option<FederationStewardTakeoverLease>,
    ),
    TaskStoreError,
> {
    let Some(lease) = lease_by_id(transaction, lease_id)? else {
        return Ok((FederationStewardTakeoverOutcome::Rejected, None));
    };
    let actor_allowed = (target_action && member.hive == lease.target_hive_id)
        || (source_action
            && member.hive == lease.source_hive_id
            && member.operator == lease.source_operator_id);
    if !actor_allowed
        || lease.state != from
        || lease.revision != expected_revision
        || lease.expires_at <= now
        || (source_action
            && authorized_stewardship(
                transaction,
                member.apiary,
                member.operator,
                lease.target_hive_id,
            )? != Some(lease.stewardship_id))
    {
        return Ok((FederationStewardTakeoverOutcome::Rejected, Some(lease)));
    }
    let revision = lease.revision.saturating_add(1);
    let acknowledged_at = if to == FederationStewardTakeoverState::Active
        && from == FederationStewardTakeoverState::Requested
    {
        Some(now)
    } else {
        lease.acknowledged_at
    };
    let expires_at = if to == FederationStewardTakeoverState::Active {
        now.saturating_add(ACTIVE_LIFETIME_SECONDS)
    } else {
        lease.expires_at
    };
    let ended_at = (!to.is_open()).then_some(now);
    transaction.execute(
        "UPDATE apiary_steward_takeover_leases
         SET state = ?1, revision = ?2, acknowledged_at = ?3, expires_at = ?4,
             ended_at = ?5, updated_at = ?6
         WHERE lease_id = ?7",
        params![
            to.to_string(),
            revision,
            acknowledged_at,
            expires_at,
            ended_at,
            now,
            lease.id.to_string()
        ],
    )?;
    Ok((
        FederationStewardTakeoverOutcome::Applied,
        Some(FederationStewardTakeoverLease {
            state: to,
            revision,
            acknowledged_at,
            expires_at,
            ended_at,
            ..lease
        }),
    ))
}

fn expire_open_leases(
    transaction: &Transaction<'_>,
    apiary_id: swarm_domain::ApiaryId,
    now: i64,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "UPDATE apiary_steward_takeover_leases
         SET state = 'expired', revision = revision + 1, ended_at = ?1, updated_at = ?1
         WHERE apiary_id = ?2 AND state IN ('requested','active') AND expires_at <= ?1",
        params![now, apiary_id.to_string()],
    )?;
    Ok(())
}

fn authorized_stewardship(
    transaction: &Transaction<'_>,
    apiary_id: swarm_domain::ApiaryId,
    operator_id: swarm_domain::OperatorId,
    hive_id: HiveId,
) -> Result<Option<StewardshipId>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT s.id FROM stewardships s
             JOIN stewardship_hive_grants h ON h.stewardship_id = s.id AND h.hive_id = ?3
             JOIN stewardship_capability_grants c ON c.stewardship_id = s.id AND c.capability = 'takeover'
             WHERE s.apiary_id = ?1 AND s.steward_operator_id = ?2 AND s.revoked_at IS NULL
             ORDER BY s.created_at DESC LIMIT 1",
            params![apiary_id.to_string(), operator_id.to_string(), hive_id.to_string()],
            |row| parse_domain_id::<StewardshipId>(&row.get::<_, String>(0)?),
        )
        .optional()
        .map_err(Into::into)
}

fn active_member_hive(
    transaction: &Transaction<'_>,
    apiary_id: swarm_domain::ApiaryId,
    hive_id: HiveId,
) -> Result<bool, TaskStoreError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM apiary_federation_memberships
         WHERE apiary_id = ?1 AND member_hive_id = ?2 AND state = 'active')",
            params![apiary_id.to_string(), hive_id.to_string()],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn open_lease_for_target(
    transaction: &Transaction<'_>,
    apiary_id: swarm_domain::ApiaryId,
    target_hive_id: HiveId,
) -> Result<Option<FederationStewardTakeoverLease>, TaskStoreError> {
    let mut leases = read_leases(
        transaction,
        "apiary_steward_takeover_leases",
        "WHERE apiary_id = ?1 AND target_hive_id = ?2 AND state IN ('requested','active') LIMIT 1",
        params![apiary_id.to_string(), target_hive_id.to_string()],
    )?;
    Ok(leases.pop())
}

fn lease_by_id(
    transaction: &Transaction<'_>,
    lease_id: FederationStewardTakeoverLeaseId,
) -> Result<Option<FederationStewardTakeoverLease>, TaskStoreError> {
    let mut leases = read_leases(
        transaction,
        "apiary_steward_takeover_leases",
        "WHERE lease_id = ?1 LIMIT 1",
        [lease_id.to_string()],
    )?;
    Ok(leases.pop())
}

fn insert_keeper_lease(
    transaction: &Transaction<'_>,
    lease: &FederationStewardTakeoverLease,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "INSERT INTO apiary_steward_takeover_leases
            (lease_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
             stewardship_id, reason, state, revision, requested_at, acknowledged_at,
             expires_at, ended_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?10)",
        params![
            lease.id.to_string(),
            lease.apiary_id.to_string(),
            lease.source_hive_id.to_string(),
            lease.target_hive_id.to_string(),
            lease.source_operator_id.to_string(),
            lease.stewardship_id.to_string(),
            lease.reason,
            lease.state.to_string(),
            lease.revision,
            lease.requested_at,
            lease.acknowledged_at,
            lease.expires_at,
            lease.ended_at,
        ],
    )?;
    Ok(())
}

fn insert_local_lease(
    transaction: &Transaction<'_>,
    lease: &FederationStewardTakeoverLease,
    now: i64,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "INSERT INTO local_federation_steward_takeover_leases
            (lease_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
             stewardship_id, reason, state, revision, requested_at, acknowledged_at,
             expires_at, ended_at, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            lease.id.to_string(),
            lease.apiary_id.to_string(),
            lease.source_hive_id.to_string(),
            lease.target_hive_id.to_string(),
            lease.source_operator_id.to_string(),
            lease.stewardship_id.to_string(),
            lease.reason,
            lease.state.to_string(),
            lease.revision,
            lease.requested_at,
            lease.acknowledged_at,
            lease.expires_at,
            lease.ended_at,
            now,
        ],
    )?;
    Ok(())
}

fn read_leases<P: rusqlite::Params>(
    connection: &rusqlite::Connection,
    table: &str,
    suffix: &str,
    parameters: P,
) -> Result<Vec<FederationStewardTakeoverLease>, TaskStoreError> {
    debug_assert!(matches!(
        table,
        "apiary_steward_takeover_leases" | "local_federation_steward_takeover_leases"
    ));
    let sql = format!(
        "SELECT lease_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
                stewardship_id, reason, state, revision, requested_at, acknowledged_at,
                expires_at, ended_at FROM {table} {suffix}"
    );
    let mut statement = connection.prepare(&sql)?;
    statement
        .query_map(parameters, takeover_lease_from_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn takeover_lease_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationStewardTakeoverLease> {
    Ok(FederationStewardTakeoverLease {
        id: parse_domain_id(&row.get::<_, String>(0)?)?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        source_hive_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        target_hive_id: parse_domain_id(&row.get::<_, String>(3)?)?,
        source_operator_id: parse_domain_id(&row.get::<_, String>(4)?)?,
        stewardship_id: parse_domain_id(&row.get::<_, String>(5)?)?,
        reason: row.get(6)?,
        state: FederationStewardTakeoverState::from_str(&row.get::<_, String>(7)?)
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        revision: row.get(8)?,
        requested_at: row.get(9)?,
        acknowledged_at: row.get(10)?,
        expires_at: row.get(11)?,
        ended_at: row.get(12)?,
    })
}

fn takeover_outbox_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationStewardTakeoverOutboxEntry> {
    let command_json: String = row.get(0)?;
    let receipt_json: Option<String> = row.get(4)?;
    Ok(FederationStewardTakeoverOutboxEntry {
        command: serde_json::from_str(&command_json).map_err(|_| rusqlite::Error::InvalidQuery)?,
        state: FederationStewardTakeoverOutboxState::from_str(&row.get::<_, String>(1)?)
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        attempt_count: row.get(2)?,
        last_attempt_at: row.get(3)?,
        receipt: receipt_json
            .map(|value| serde_json::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()?,
    })
}

fn require_local_lease(
    transaction: &Transaction<'_>,
    lease_id: FederationStewardTakeoverLeaseId,
    local_hive_id: HiveId,
    source: bool,
    expected_revision: u64,
    expected_state: FederationStewardTakeoverState,
) -> Result<(), TaskStoreError> {
    let found = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_federation_steward_takeover_leases
         WHERE lease_id = ?1 AND revision = ?2 AND state = ?5
         AND CASE WHEN ?3 THEN source_hive_id = ?4 ELSE target_hive_id = ?4 END)",
        params![
            lease_id.to_string(),
            expected_revision,
            source,
            local_hive_id.to_string(),
            expected_state.to_string(),
        ],
        |row| row.get::<_, bool>(0),
    )?;
    found
        .then_some(())
        .ok_or(TaskStoreError::InvalidFederationStewardTakeover)
}

fn validate_command(
    command: &FederationStewardTakeoverCommand,
    now: i64,
) -> Result<(), TaskStoreError> {
    if command.created_at <= 0 || command.created_at > now.saturating_add(300) {
        return Err(TaskStoreError::InvalidFederationStewardTakeover);
    }
    match &command.action {
        FederationStewardTakeoverAction::Request {
            reason,
            relay_protocol_version,
            terminal_protocol_version,
            ..
        } if !valid_reason(reason)
            || *relay_protocol_version != STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION
            || *terminal_protocol_version != STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION =>
        {
            Err(TaskStoreError::InvalidFederationStewardTakeover)
        }
        FederationStewardTakeoverAction::Acknowledge {
            expected_revision,
            relay_protocol_version,
            terminal_protocol_version,
            ..
        } if *expected_revision == 0
            || *relay_protocol_version != STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION
            || *terminal_protocol_version != STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION =>
        {
            Err(TaskStoreError::InvalidFederationStewardTakeover)
        }
        FederationStewardTakeoverAction::Renew {
            expected_revision, ..
        }
        | FederationStewardTakeoverAction::Release {
            expected_revision, ..
        } if *expected_revision == 0 => Err(TaskStoreError::InvalidFederationStewardTakeover),
        FederationStewardTakeoverAction::Reclaim {
            expected_revision,
            reason,
            ..
        } if *expected_revision == 0 || !valid_reason(reason) => {
            Err(TaskStoreError::InvalidFederationStewardTakeover)
        }
        _ => Ok(()),
    }
}

fn valid_reason(reason: &str) -> bool {
    !reason.trim().is_empty()
        && reason.len() <= MAX_TAKEOVER_REASON_BYTES
        && !reason
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn valid_lease(lease: &FederationStewardTakeoverLease, now: i64) -> bool {
    valid_reason(&lease.reason)
        && lease.revision > 0
        && lease.requested_at > 0
        && lease.requested_at <= now.saturating_add(300)
        && lease.expires_at > lease.requested_at
        && match lease.state {
            FederationStewardTakeoverState::Requested => lease.acknowledged_at.is_none(),
            FederationStewardTakeoverState::Active
            | FederationStewardTakeoverState::Released
            | FederationStewardTakeoverState::Reclaimed => lease.acknowledged_at.is_some(),
            FederationStewardTakeoverState::Expired => true,
        }
        && lease.state.is_open() == lease.ended_at.is_none()
}

pub(super) fn migrate_federation_steward_takeovers(
    transaction: &Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_steward_takeover_leases (
             lease_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             source_hive_id TEXT NOT NULL REFERENCES hives(id), target_hive_id TEXT NOT NULL REFERENCES hives(id),
             source_operator_id TEXT NOT NULL REFERENCES operators(id), stewardship_id TEXT NOT NULL REFERENCES stewardships(id),
             reason TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('requested','active','released','reclaimed','expired')),
             revision INTEGER NOT NULL, requested_at INTEGER NOT NULL, acknowledged_at INTEGER,
             expires_at INTEGER NOT NULL, ended_at INTEGER, updated_at INTEGER NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_open_takeover_per_target
             ON apiary_steward_takeover_leases(apiary_id, target_hive_id)
             WHERE state IN ('requested','active');
         CREATE INDEX IF NOT EXISTS apiary_steward_takeover_participants
             ON apiary_steward_takeover_leases(apiary_id, source_hive_id, target_hive_id, requested_at DESC);
         CREATE TABLE IF NOT EXISTS apiary_steward_takeover_commands (
             command_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             member_node_id TEXT NOT NULL, member_hive_id TEXT NOT NULL, member_operator_id TEXT NOT NULL,
             command_json TEXT NOT NULL, outcome TEXT NOT NULL CHECK (outcome IN ('applied','rejected','conflict')),
             lease_id TEXT, receipt_json TEXT NOT NULL, processed_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS apiary_steward_takeover_audit
             ON apiary_steward_takeover_commands(apiary_id, processed_at DESC);
         CREATE TABLE IF NOT EXISTS local_federation_steward_takeover_leases (
             lease_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL, source_hive_id TEXT NOT NULL,
             target_hive_id TEXT NOT NULL, source_operator_id TEXT NOT NULL, stewardship_id TEXT NOT NULL,
             reason TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('requested','active','released','reclaimed','expired')),
             revision INTEGER NOT NULL, requested_at INTEGER NOT NULL, acknowledged_at INTEGER,
             expires_at INTEGER NOT NULL, ended_at INTEGER, synced_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS local_federation_steward_takeover_commands (
             command_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL, command_json TEXT NOT NULL,
             state TEXT NOT NULL CHECK (state IN ('queued','applied','rejected','conflict')),
             attempt_count INTEGER NOT NULL, last_attempt_at INTEGER, receipt_json TEXT,
             created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS local_federation_steward_takeover_queue
             ON local_federation_steward_takeover_commands(state, created_at, command_id);
         PRAGMA user_version = 60;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        FederationJoinAcceptance, FederationJoinReadiness, JiraConnectionState, SharedWorkBackend,
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

    fn setup_takeover(
        now: i64,
    ) -> (
        TaskStore,
        TaskStore,
        FederationJoinAcceptance,
        TaskStore,
        FederationJoinAcceptance,
    ) {
        let keeper = TaskStore::in_memory().expect("keeper");
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .expect("apiary");
        let (steward, steward_acceptance) = join_member(&keeper, now + 10);
        let (target, target_acceptance) = join_member(&keeper, now + 30);
        let steward_identity = steward.local_hive_identity().expect("identity");
        keeper
            .set_stewardship(
                steward_identity.operator.id,
                &[target_acceptance.receipt.payload.member_hive_id],
                &[StewardCapability::Observe, StewardCapability::Takeover],
                now + 50,
            )
            .expect("stewardship");
        let scope = keeper
            .federation_stewardship_snapshot(&steward_acceptance.node_credential, now + 51)
            .expect("scope");
        steward
            .apply_federation_stewardship_snapshot(&scope, now + 52)
            .expect("projection");
        (
            keeper,
            steward,
            steward_acceptance,
            target,
            target_acceptance,
        )
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn takeover_requires_target_acknowledgement_and_target_reclaim_wins() {
        let now = 700_000;
        let (keeper, steward, steward_acceptance, target, target_acceptance) = setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;

        let request = steward
            .queue_federation_steward_takeover(
                target_hive_id,
                "The release is blocked while the operator is unavailable.",
                now + 53,
            )
            .expect("journal request");
        assert_eq!(
            steward
                .pending_federation_steward_takeovers(20)
                .expect("outbox"),
            vec![request.clone()]
        );
        let requested = keeper
            .apply_federation_steward_takeover_command(
                &steward_acceptance.node_credential,
                &request.command,
                now + 54,
            )
            .expect("request");
        assert_eq!(requested.outcome, FederationStewardTakeoverOutcome::Applied);
        assert_eq!(
            requested.lease.as_ref().map(|lease| lease.state),
            Some(FederationStewardTakeoverState::Requested)
        );
        assert!(requested.lease.as_ref().unwrap().acknowledged_at.is_none());
        assert_eq!(
            keeper
                .apply_federation_steward_takeover_command(
                    &steward_acceptance.node_credential,
                    &request.command,
                    now + 55,
                )
                .expect("retry"),
            requested
        );
        steward
            .apply_federation_steward_takeover_receipt(&requested, now + 56)
            .expect("source receipt");

        let target_inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 57)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        let queen_session = swarm_domain::WorkerSessionId::new();
        target
            .bind_worker_session(queen.id, queen_session)
            .expect("bind queen");
        target
            .apply_federation_steward_takeover_inbox(&target_inbox, now + 58)
            .expect("target projection");
        assert_eq!(
            target.active_queen_session_id().expect("queen session"),
            Some(queen_session)
        );
        assert!(
            !target
                .worker_accepts_injection(queen.id, now + 58)
                .expect("automation guard"),
            "a requested installed takeover pauses competing Queen automation"
        );
        let lease = target_inbox.leases.first().expect("requested lease");
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(lease.id, lease.revision, now + 59)
            .expect("journal acknowledgement");
        let active = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 60,
            )
            .expect("acknowledge");
        assert_eq!(active.outcome, FederationStewardTakeoverOutcome::Applied);
        let active_lease = active.lease.as_ref().expect("active lease");
        assert_eq!(active_lease.state, FederationStewardTakeoverState::Active);
        assert_eq!(active_lease.revision, 2);
        assert_eq!(active_lease.acknowledged_at, Some(now + 60));
        assert_eq!(active_lease.expires_at, now + 60 + ACTIVE_LIFETIME_SECONDS);
        target
            .apply_federation_steward_takeover_receipt(&active, now + 61)
            .expect("target receipt");

        let refreshed = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 62)
            .expect("active poll");
        target
            .apply_federation_steward_takeover_inbox(&refreshed, now + 63)
            .expect("active projection");
        let reclaim = target
            .queue_federation_steward_takeover_reclaim(
                active_lease.id,
                active_lease.revision,
                "The local operator returned.",
                now + 64,
            )
            .expect("journal reclaim");
        let local_after_reclaim = target
            .federation_steward_takeover_local_state()
            .expect("local reclaim projection");
        assert_eq!(
            local_after_reclaim.leases[0].state,
            FederationStewardTakeoverState::Reclaimed,
            "local authority closes before Keeper observes the outbound command"
        );
        let reclaimed = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &reclaim.command,
                now + 65,
            )
            .expect("reclaim");
        assert_eq!(reclaimed.outcome, FederationStewardTakeoverOutcome::Applied);
        assert_eq!(
            reclaimed.lease.as_ref().map(|lease| lease.state),
            Some(FederationStewardTakeoverState::Reclaimed)
        );
        assert_eq!(reclaimed.lease.as_ref().unwrap().ended_at, Some(now + 65));
    }

    #[test]
    fn concurrent_takeover_conflicts_and_expired_request_frees_the_target() {
        let now = 800_000;
        let (keeper, steward, acceptance, _target, target_acceptance) = setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let first = steward
            .queue_federation_steward_takeover(target_hive_id, "First reason", now + 53)
            .expect("first");
        let first_receipt = keeper
            .apply_federation_steward_takeover_command(
                &acceptance.node_credential,
                &first.command,
                now + 54,
            )
            .expect("first apply");
        let second = FederationStewardTakeoverCommand {
            id: FederationStewardTakeoverCommandId::new(),
            apiary_id: first.command.apiary_id,
            action: FederationStewardTakeoverAction::Request {
                target_hive_id,
                reason: "Second reason".to_owned(),
                relay_protocol_version: STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION,
                terminal_protocol_version: STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION,
            },
            created_at: now + 55,
        };
        let conflict = keeper
            .apply_federation_steward_takeover_command(
                &acceptance.node_credential,
                &second,
                now + 55,
            )
            .expect("conflict receipt");
        assert_eq!(conflict.outcome, FederationStewardTakeoverOutcome::Conflict);
        assert!(conflict.lease.is_none());

        let after_expiry = FederationStewardTakeoverCommand {
            id: FederationStewardTakeoverCommandId::new(),
            created_at: first_receipt.lease.as_ref().unwrap().expires_at + 1,
            ..second
        };
        let replacement = keeper
            .apply_federation_steward_takeover_command(
                &acceptance.node_credential,
                &after_expiry,
                first_receipt.lease.as_ref().unwrap().expires_at + 1,
            )
            .expect("replacement");
        assert_eq!(
            replacement.outcome,
            FederationStewardTakeoverOutcome::Applied
        );
        assert_ne!(
            replacement.lease.as_ref().map(|lease| lease.id),
            first_receipt.lease.as_ref().map(|lease| lease.id)
        );
    }

    #[test]
    fn revoked_scope_rejects_request_and_source_renewal() {
        let now = 900_000;
        let (keeper, steward, acceptance, target, target_acceptance) = setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let request = steward
            .queue_federation_steward_takeover(target_hive_id, "Need control", now + 53)
            .expect("request");
        let requested = keeper
            .apply_federation_steward_takeover_command(
                &acceptance.node_credential,
                &request.command,
                now + 54,
            )
            .expect("apply request");
        let target_inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 55)
            .expect("target inbox");
        target
            .apply_federation_steward_takeover_inbox(&target_inbox, now + 56)
            .expect("target projection");
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(
                requested.lease.as_ref().unwrap().id,
                1,
                now + 57,
            )
            .expect("ack");
        let active = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 58,
            )
            .expect("active");
        let active_source_inbox = keeper
            .federation_steward_takeover_inbox(&acceptance.node_credential, now + 59)
            .expect("source inbox");
        steward
            .apply_federation_steward_takeover_inbox(&active_source_inbox, now + 60)
            .expect("source projection");
        let renewal = steward
            .queue_federation_steward_takeover_renewal(
                active.lease.as_ref().unwrap().id,
                active.lease.as_ref().unwrap().revision,
                now + 61,
            )
            .expect("journal renewal");
        let scope = keeper
            .federation_stewardship_snapshot(&acceptance.node_credential, now + 62)
            .expect("scope");
        keeper
            .revoke_stewardship(scope.stewardship.as_ref().unwrap().id, now + 63)
            .expect("revoke");
        let rejected = keeper
            .apply_federation_steward_takeover_command(
                &acceptance.node_credential,
                &renewal.command,
                now + 64,
            )
            .expect("durable rejection");
        assert_eq!(rejected.outcome, FederationStewardTakeoverOutcome::Rejected);
        assert_eq!(
            rejected.lease.as_ref().map(|lease| lease.state),
            Some(FederationStewardTakeoverState::Active)
        );
    }
}
