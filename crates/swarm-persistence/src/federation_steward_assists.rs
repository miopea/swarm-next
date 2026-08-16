use std::str::FromStr;

use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, FederationStewardAssistAction, FederationStewardAssistCommand,
    FederationStewardAssistCommandId, FederationStewardAssistInbox,
    FederationStewardAssistLocalState, FederationStewardAssistOutboxEntry,
    FederationStewardAssistOutboxState, FederationStewardAssistOutcome,
    FederationStewardAssistReceipt, FederationStewardAssistRequest,
    FederationStewardAssistRequestId, FederationStewardAssistState, HiveId, LocalApiaryContext,
    LocalApiaryRole, StewardCapability, StewardshipId,
};

use super::{
    TaskStore, TaskStoreError,
    federation::{MemberCredentialContext, authenticate_member_credential, decode_node_credential},
    insert_control_room_event, parse_domain_id,
};

pub const MAX_FEDERATION_STEWARD_ASSIST_BATCH: usize = 20;
const MAX_LOCAL_ASSIST_OUTBOX: usize = 256;
const MAX_KEEPER_ASSIST_COMMANDS: usize = 10_000;
const MAX_ASSIST_MESSAGE_BYTES: usize = 2_000;

impl TaskStore {
    /// Applies one credential-bound Assist request or response at Keeper.
    /// Every retry returns the original receipt, and every denial is durable.
    ///
    /// # Errors
    ///
    /// Returns an error when the credential or command is invalid, the caller
    /// lacks the current Assist scope, the target is unavailable, or storage
    /// cannot durably record the result.
    pub fn apply_federation_steward_assist_command(
        &self,
        node_credential: &str,
        command: &FederationStewardAssistCommand,
        now: i64,
    ) -> Result<FederationStewardAssistReceipt, TaskStoreError> {
        validate_command(command, now)?;
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        if command.apiary_id != member.apiary {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        }
        let command_json = serde_json::to_string(command)
            .map_err(|_| TaskStoreError::InvalidFederationStewardAssist)?;
        if let Some((node_id, prior_command, receipt_json)) = transaction
            .query_row(
                "SELECT member_node_id, command_json, receipt_json
                 FROM apiary_steward_assist_commands WHERE command_id = ?1",
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
                return Err(TaskStoreError::InvalidFederationStewardAssist);
            }
            return serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationStewardAssist);
        }
        let command_count = transaction.query_row(
            "SELECT COUNT(*) FROM apiary_steward_assist_commands WHERE apiary_id = ?1",
            [member.apiary.to_string()],
            |row| row.get::<_, usize>(0),
        )?;
        if command_count >= MAX_KEEPER_ASSIST_COMMANDS {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        }

        let (outcome, stewardship_id, request) =
            apply_authenticated_command(&transaction, &member, command, now)?;
        let receipt = FederationStewardAssistReceipt {
            command_id: command.id,
            outcome,
            stewardship_id,
            request,
            processed_at: now,
        };
        let receipt_json = serde_json::to_string(&receipt)
            .map_err(|_| TaskStoreError::InvalidFederationStewardAssist)?;
        transaction.execute(
            "INSERT INTO apiary_steward_assist_commands
                (command_id, apiary_id, member_node_id, member_hive_id,
                 member_operator_id, command_json, outcome, request_id,
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
                receipt
                    .request
                    .as_ref()
                    .map(|request| request.id.to_string()),
                receipt_json,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    /// Returns only assistance addressed to the authenticated Member Hive.
    ///
    /// # Errors
    ///
    /// Returns an error when authentication fails or the inbox cannot be read.
    pub fn federation_steward_assist_inbox(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationStewardAssistInbox, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let connection = self.connection()?;
        let member = authenticate_member_credential(&connection, &identity, &credential, now)?;
        let mut statement = connection.prepare(
            "SELECT request_id, apiary_id, source_hive_id, target_hive_id,
                    message, state, created_at, resolved_at
             FROM apiary_steward_assist_requests
             WHERE apiary_id = ?1 AND (target_hive_id = ?2 OR source_hive_id = ?2)
             ORDER BY created_at DESC, request_id DESC LIMIT 100",
        )?;
        let requests = statement
            .query_map(
                params![member.apiary.to_string(), member.hive.to_string()],
                request_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FederationStewardAssistInbox {
            requests,
            generated_at: now,
        })
    }

    /// Queues a Steward request before any network I/O.
    ///
    /// # Errors
    ///
    /// Returns an error when this Hive is not an authorized Steward for the
    /// target, the request is invalid, the queue is full, or storage fails.
    pub fn queue_federation_steward_assist(
        &self,
        target_hive_id: HiveId,
        message: &str,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, TaskStoreError> {
        self.queue_local_assist(
            FederationStewardAssistAction::Request {
                target_hive_id,
                message: message.trim().to_owned(),
            },
            now,
        )
    }

    /// Queues the target operator's explicit response. A pending local inbox
    /// record is required; accepting never opens a terminal or starts work.
    ///
    /// # Errors
    ///
    /// Returns an error when no matching pending request exists, the response
    /// is invalid or already queued, the queue is full, or storage fails.
    pub fn queue_federation_steward_assist_response(
        &self,
        request_id: FederationStewardAssistRequestId,
        decision: FederationStewardAssistState,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, TaskStoreError> {
        if decision == FederationStewardAssistState::Pending {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        }
        self.queue_local_assist(
            FederationStewardAssistAction::Respond {
                request_id,
                decision,
            },
            now,
        )
    }

    fn queue_local_assist(
        &self,
        action: FederationStewardAssistAction,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, TaskStoreError> {
        self.require_local_federation_member()?;
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated {
            apiary,
            local_role: LocalApiaryRole::Member,
        } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        };
        let command = FederationStewardAssistCommand {
            id: FederationStewardAssistCommandId::new(),
            apiary_id: apiary.id,
            action,
            created_at: now,
        };
        validate_command(&command, now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        match &command.action {
            FederationStewardAssistAction::Request { target_hive_id, .. } => {
                let snapshot_json = transaction.query_row(
                    "SELECT snapshot_json FROM local_federation_stewardship WHERE singleton = 1",
                    [], |row| row.get::<_, String>(0),
                ).optional()?.ok_or(TaskStoreError::StewardActionDenied)?;
                let snapshot: swarm_domain::FederationStewardshipSnapshot =
                    serde_json::from_str(&snapshot_json)
                        .map_err(|_| TaskStoreError::InvalidFederationStewardAssist)?;
                if snapshot.apiary_id != apiary.id
                    || snapshot.member_operator_id != identity.operator.id
                    || !snapshot.stewardship.as_ref().is_some_and(|scope| {
                        scope.allows(*target_hive_id, StewardCapability::Assist)
                    })
                {
                    return Err(TaskStoreError::StewardActionDenied);
                }
            }
            FederationStewardAssistAction::Respond { request_id, .. } => {
                let pending = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM local_federation_steward_assist_requests
                     WHERE request_id = ?1 AND target_hive_id = ?2 AND state = 'pending')",
                    params![request_id.to_string(), identity.hive.id.to_string()],
                    |row| row.get::<_, bool>(0),
                )?;
                if !pending {
                    return Err(TaskStoreError::InvalidFederationStewardAssist);
                }
                let mut statement = transaction.prepare(
                    "SELECT command_json FROM local_federation_steward_assist_commands WHERE state = 'queued'",
                )?;
                let queued_commands = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                if queued_commands.iter().any(|serialized| {
                    serde_json::from_str::<FederationStewardAssistCommand>(serialized).is_ok_and(|candidate| {
                        matches!(candidate.action, FederationStewardAssistAction::Respond { request_id: queued_id, .. } if queued_id == *request_id)
                    })
                }) {
                    return Err(TaskStoreError::InvalidFederationStewardAssist);
                }
            }
        }
        let queued = transaction.query_row(
            "SELECT COUNT(*) FROM local_federation_steward_assist_commands WHERE state = 'queued'",
            [],
            |row| row.get::<_, usize>(0),
        )?;
        if queued >= MAX_LOCAL_ASSIST_OUTBOX {
            return Err(TaskStoreError::FederationStewardAssistQueueFull);
        }
        let command_json = serde_json::to_string(&command)
            .map_err(|_| TaskStoreError::InvalidFederationStewardAssist)?;
        transaction.execute(
            "INSERT INTO local_federation_steward_assist_commands
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
        Ok(FederationStewardAssistOutboxEntry {
            command,
            state: FederationStewardAssistOutboxState::Queued,
            attempt_count: 0,
            last_attempt_at: None,
            receipt: None,
        })
    }

    /// Returns the bounded batch of Assist commands awaiting delivery.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid limit or when storage cannot be read.
    pub fn pending_federation_steward_assists(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardAssistOutboxEntry>, TaskStoreError> {
        if limit == 0 || limit > MAX_FEDERATION_STEWARD_ASSIST_BATCH {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        }
        self.read_assist_outbox(
            "WHERE state = 'queued' ORDER BY created_at, command_id LIMIT ?1",
            Some(limit),
        )
    }

    /// Records one delivery attempt for a still-queued Assist command.
    ///
    /// # Errors
    ///
    /// Returns an error when the command is no longer queued or storage fails.
    pub fn record_federation_steward_assist_attempt(
        &self,
        command_id: FederationStewardAssistCommandId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let changed = self.connection()?.execute(
            "UPDATE local_federation_steward_assist_commands SET attempt_count = attempt_count + 1,
             last_attempt_at = ?1, updated_at = ?1 WHERE command_id = ?2 AND state = 'queued'",
            params![now, command_id.to_string()],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(TaskStoreError::InvalidFederationStewardAssist)
        }
    }

    /// Applies Keeper's durable receipt to the matching local outbox entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt is invalid, has no queued command, or
    /// storage cannot update the entry.
    pub fn apply_federation_steward_assist_receipt(
        &self,
        receipt: &FederationStewardAssistReceipt,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, TaskStoreError> {
        let state = match receipt.outcome {
            FederationStewardAssistOutcome::Applied => FederationStewardAssistOutboxState::Applied,
            FederationStewardAssistOutcome::Rejected => {
                FederationStewardAssistOutboxState::Rejected
            }
        };
        let serialized = serde_json::to_string(receipt)
            .map_err(|_| TaskStoreError::InvalidFederationStewardAssist)?;
        let changed = self.connection()?.execute(
            "UPDATE local_federation_steward_assist_commands SET state = ?1, receipt_json = ?2,
             updated_at = ?3 WHERE command_id = ?4 AND state = 'queued'",
            params![
                state.to_string(),
                serialized,
                now,
                receipt.command_id.to_string()
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        }
        self.read_assist_outbox("ORDER BY created_at DESC LIMIT 100", None)?
            .into_iter()
            .find(|entry| entry.command.id == receipt.command_id)
            .ok_or(TaskStoreError::InvalidFederationStewardAssist)
    }

    /// Replaces the local inbox with Keeper's bounded target-Hive projection.
    ///
    /// # Errors
    ///
    /// Returns an error when the projection is invalid for this Member Hive or
    /// cannot be stored atomically.
    pub fn apply_federation_steward_assist_inbox(
        &self,
        inbox: &FederationStewardAssistInbox,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated {
            apiary,
            local_role: LocalApiaryRole::Member,
        } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        };
        if inbox.generated_at > now.saturating_add(300)
            || inbox.requests.len() > 100
            || inbox.requests.iter().any(|request| {
                request.apiary_id != apiary.id
                    || (request.target_hive_id != identity.hive.id
                        && request.source_hive_id != identity.hive.id)
                    || !valid_request(request, now)
            })
        {
            return Err(TaskStoreError::InvalidFederationStewardAssist);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let existing = {
            let mut statement = transaction.prepare(
                "SELECT request_id, apiary_id, source_hive_id, target_hive_id,
                        message, state, created_at, resolved_at
                 FROM local_federation_steward_assist_requests
                 ORDER BY created_at DESC, request_id DESC",
            )?;
            statement
                .query_map([], request_from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        if existing == inbox.requests {
            return Ok(());
        }
        transaction.execute("DELETE FROM local_federation_steward_assist_requests", [])?;
        for request in &inbox.requests {
            transaction.execute(
                "INSERT INTO local_federation_steward_assist_requests
                    (request_id, apiary_id, source_hive_id, target_hive_id, message,
                     state, created_at, resolved_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    request.id.to_string(),
                    request.apiary_id.to_string(),
                    request.source_hive_id.to_string(),
                    request.target_hive_id.to_string(),
                    request.message,
                    request.state.to_string(),
                    request.created_at,
                    request.resolved_at,
                    now
                ],
            )?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the local operator-facing Assist inbox, sent list, and outbox.
    ///
    /// # Errors
    ///
    /// Returns an error when local identity or stored Assist state is invalid.
    pub fn federation_steward_assist_local_state(
        &self,
    ) -> Result<FederationStewardAssistLocalState, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT request_id, apiary_id, source_hive_id, target_hive_id,
                    message, state, created_at, resolved_at
             FROM local_federation_steward_assist_requests ORDER BY created_at DESC, request_id DESC",
        )?;
        let requests = statement
            .query_map([], request_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        Ok(FederationStewardAssistLocalState {
            incoming: requests
                .iter()
                .filter(|request| request.target_hive_id == identity.hive.id)
                .cloned()
                .collect(),
            sent: requests
                .into_iter()
                .filter(|request| request.source_hive_id == identity.hive.id)
                .collect(),
            outbox: self.read_assist_outbox("ORDER BY created_at DESC LIMIT 100", None)?,
        })
    }

    fn read_assist_outbox(
        &self,
        suffix: &str,
        limit: Option<usize>,
    ) -> Result<Vec<FederationStewardAssistOutboxEntry>, TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json FROM local_federation_steward_assist_commands {suffix}"
        );
        let mut statement = connection.prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| outbox_from_row(row);
        let rows = if let Some(limit) = limit {
            statement
                .query_map(
                    [i64::try_from(limit)
                        .map_err(|_| TaskStoreError::InvalidFederationStewardAssist)?],
                    map,
                )?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            statement
                .query_map([], map)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }
}

fn apply_authenticated_command(
    transaction: &Transaction<'_>,
    member: &MemberCredentialContext,
    command: &FederationStewardAssistCommand,
    now: i64,
) -> Result<
    (
        FederationStewardAssistOutcome,
        Option<StewardshipId>,
        Option<FederationStewardAssistRequest>,
    ),
    TaskStoreError,
> {
    match &command.action {
        FederationStewardAssistAction::Request {
            target_hive_id,
            message,
        } => {
            let stewardship_id = authorized_stewardship(
                transaction,
                member.apiary,
                member.operator,
                *target_hive_id,
                "assist",
            )?;
            let target_active = active_member_hive(transaction, member.apiary, *target_hive_id)?;
            if let (Some(stewardship_id), true) = (stewardship_id, target_active) {
                let request = FederationStewardAssistRequest {
                    id: FederationStewardAssistRequestId::new(),
                    apiary_id: member.apiary,
                    source_hive_id: member.hive,
                    target_hive_id: *target_hive_id,
                    message: message.trim().to_owned(),
                    state: FederationStewardAssistState::Pending,
                    created_at: now,
                    resolved_at: None,
                };
                insert_request(transaction, &request, stewardship_id, member.operator)?;
                Ok((
                    FederationStewardAssistOutcome::Applied,
                    Some(stewardship_id),
                    Some(request),
                ))
            } else {
                Ok((FederationStewardAssistOutcome::Rejected, None, None))
            }
        }
        FederationStewardAssistAction::Respond {
            request_id,
            decision,
        } => {
            let existing = request_by_id(transaction, *request_id)?;
            if let Some(mut request) = existing.filter(|request| {
                request.apiary_id == member.apiary
                    && request.target_hive_id == member.hive
                    && request.state == FederationStewardAssistState::Pending
            }) {
                transaction.execute(
                    "UPDATE apiary_steward_assist_requests
                     SET state = ?1, resolved_at = ?2, updated_at = ?2
                     WHERE request_id = ?3 AND state = 'pending'",
                    params![decision.to_string(), now, request.id.to_string()],
                )?;
                request.state = *decision;
                request.resolved_at = Some(now);
                Ok((FederationStewardAssistOutcome::Applied, None, Some(request)))
            } else {
                Ok((FederationStewardAssistOutcome::Rejected, None, None))
            }
        }
    }
}

fn authorized_stewardship(
    transaction: &Transaction<'_>,
    apiary_id: swarm_domain::ApiaryId,
    operator_id: swarm_domain::OperatorId,
    hive_id: HiveId,
    capability: &str,
) -> Result<Option<StewardshipId>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT s.id FROM stewardships s
         JOIN stewardship_hive_grants h ON h.stewardship_id = s.id AND h.hive_id = ?3
         JOIN stewardship_capability_grants c ON c.stewardship_id = s.id AND c.capability = ?4
         WHERE s.apiary_id = ?1 AND s.steward_operator_id = ?2 AND s.revoked_at IS NULL
         ORDER BY s.created_at DESC LIMIT 1",
            params![
                apiary_id.to_string(),
                operator_id.to_string(),
                hive_id.to_string(),
                capability
            ],
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
    transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM apiary_federation_memberships WHERE apiary_id = ?1 AND member_hive_id = ?2 AND state = 'active')",
        params![apiary_id.to_string(), hive_id.to_string()], |row| row.get(0),
    ).map_err(Into::into)
}

fn insert_request(
    transaction: &Transaction<'_>,
    request: &FederationStewardAssistRequest,
    stewardship_id: StewardshipId,
    operator_id: swarm_domain::OperatorId,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "INSERT INTO apiary_steward_assist_requests
            (request_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
             stewardship_id, message, state, created_at, resolved_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?9)",
        params![
            request.id.to_string(),
            request.apiary_id.to_string(),
            request.source_hive_id.to_string(),
            request.target_hive_id.to_string(),
            operator_id.to_string(),
            stewardship_id.to_string(),
            request.message,
            request.state.to_string(),
            request.created_at
        ],
    )?;
    Ok(())
}

fn request_by_id(
    transaction: &Transaction<'_>,
    id: FederationStewardAssistRequestId,
) -> Result<Option<FederationStewardAssistRequest>, TaskStoreError> {
    transaction.query_row(
        "SELECT request_id, apiary_id, source_hive_id, target_hive_id, message, state, created_at, resolved_at FROM apiary_steward_assist_requests WHERE request_id = ?1",
        [id.to_string()], request_from_row,
    ).optional().map_err(Into::into)
}

fn request_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FederationStewardAssistRequest> {
    Ok(FederationStewardAssistRequest {
        id: parse_domain_id(&row.get::<_, String>(0)?)?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        source_hive_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        target_hive_id: parse_domain_id(&row.get::<_, String>(3)?)?,
        message: row.get(4)?,
        state: FederationStewardAssistState::from_str(&row.get::<_, String>(5)?)
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(6)?,
        resolved_at: row.get(7)?,
    })
}

fn outbox_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationStewardAssistOutboxEntry> {
    let command_json: String = row.get(0)?;
    let receipt_json: Option<String> = row.get(4)?;
    Ok(FederationStewardAssistOutboxEntry {
        command: serde_json::from_str(&command_json).map_err(|_| rusqlite::Error::InvalidQuery)?,
        state: FederationStewardAssistOutboxState::from_str(&row.get::<_, String>(1)?)
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        attempt_count: row.get(2)?,
        last_attempt_at: row.get(3)?,
        receipt: receipt_json
            .map(|value| serde_json::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()?,
    })
}

fn validate_command(
    command: &FederationStewardAssistCommand,
    now: i64,
) -> Result<(), TaskStoreError> {
    if command.created_at <= 0 || command.created_at > now.saturating_add(300) {
        return Err(TaskStoreError::InvalidFederationStewardAssist);
    }
    match &command.action {
        FederationStewardAssistAction::Request { message, .. }
            if message.trim().is_empty()
                || message.len() > MAX_ASSIST_MESSAGE_BYTES
                || message.chars().any(|character| {
                    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
                }) =>
        {
            Err(TaskStoreError::InvalidFederationStewardAssist)
        }
        FederationStewardAssistAction::Respond {
            decision: FederationStewardAssistState::Pending,
            ..
        } => Err(TaskStoreError::InvalidFederationStewardAssist),
        _ => Ok(()),
    }
}

fn valid_request(request: &FederationStewardAssistRequest, now: i64) -> bool {
    !request.message.trim().is_empty()
        && request.message.len() <= MAX_ASSIST_MESSAGE_BYTES
        && !request
            .message
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        && request.created_at > 0
        && request.created_at <= now.saturating_add(300)
        && (request.state == FederationStewardAssistState::Pending) == request.resolved_at.is_none()
}

pub(super) fn migrate_federation_steward_assists(
    transaction: &Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_steward_assist_requests (
             request_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             source_hive_id TEXT NOT NULL REFERENCES hives(id), target_hive_id TEXT NOT NULL REFERENCES hives(id),
             source_operator_id TEXT NOT NULL REFERENCES operators(id), stewardship_id TEXT NOT NULL REFERENCES stewardships(id),
             message TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('pending','accepted','declined')),
             created_at INTEGER NOT NULL, resolved_at INTEGER, updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS apiary_steward_assist_target ON apiary_steward_assist_requests(apiary_id, target_hive_id, state, created_at DESC);
         CREATE TABLE IF NOT EXISTS apiary_steward_assist_commands (
             command_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             member_node_id TEXT NOT NULL, member_hive_id TEXT NOT NULL, member_operator_id TEXT NOT NULL,
             command_json TEXT NOT NULL, outcome TEXT NOT NULL CHECK (outcome IN ('applied','rejected')),
             request_id TEXT, receipt_json TEXT NOT NULL, processed_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS apiary_steward_assist_audit ON apiary_steward_assist_commands(apiary_id, processed_at DESC);
         CREATE TABLE IF NOT EXISTS local_federation_steward_assist_requests (
             request_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL, source_hive_id TEXT NOT NULL,
             target_hive_id TEXT NOT NULL, message TEXT NOT NULL,
             state TEXT NOT NULL CHECK (state IN ('pending','accepted','declined')),
             created_at INTEGER NOT NULL, resolved_at INTEGER, synced_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS local_federation_steward_assist_commands (
             command_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL, command_json TEXT NOT NULL,
             state TEXT NOT NULL CHECK (state IN ('queued','applied','rejected')),
             attempt_count INTEGER NOT NULL, last_attempt_at INTEGER, receipt_json TEXT,
             created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS local_federation_steward_assist_queue ON local_federation_steward_assist_commands(state, created_at, command_id);
         PRAGMA user_version = 59;",
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

    #[test]
    #[allow(clippy::too_many_lines)]
    fn assistance_is_polled_outward_and_requires_an_explicit_target_response() {
        let now = 400_000;
        let keeper = TaskStore::in_memory().expect("keeper");
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .expect("apiary");
        let (steward, steward_acceptance) = join_member(&keeper, now + 10);
        let (target, target_acceptance) = join_member(&keeper, now + 30);
        let steward_identity = steward.local_hive_identity().expect("identity");
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        keeper
            .set_stewardship(
                steward_identity.operator.id,
                &[target_hive_id],
                &[StewardCapability::Observe, StewardCapability::Assist],
                now + 50,
            )
            .expect("stewardship");
        let scope = keeper
            .federation_stewardship_snapshot(&steward_acceptance.node_credential, now + 51)
            .expect("scope");
        steward
            .apply_federation_stewardship_snapshot(&scope, now + 52)
            .expect("projection");

        let queued = steward
            .queue_federation_steward_assist(
                target_hive_id,
                "I can help unblock the shared release decision.",
                now + 53,
            )
            .expect("queue request");
        let receipt = keeper
            .apply_federation_steward_assist_command(
                &steward_acceptance.node_credential,
                &queued.command,
                now + 54,
            )
            .expect("keeper apply");
        assert_eq!(receipt.outcome, FederationStewardAssistOutcome::Applied);
        assert_eq!(
            keeper
                .apply_federation_steward_assist_command(
                    &steward_acceptance.node_credential,
                    &queued.command,
                    now + 55,
                )
                .expect("exact retry"),
            receipt
        );
        steward
            .apply_federation_steward_assist_receipt(&receipt, now + 56)
            .expect("receipt");

        let inbox = keeper
            .federation_steward_assist_inbox(&target_acceptance.node_credential, now + 57)
            .expect("outward poll");
        assert_eq!(inbox.requests.len(), 1);
        assert_eq!(inbox.requests[0].target_hive_id, target_hive_id);
        assert_eq!(
            inbox.requests[0].state,
            FederationStewardAssistState::Pending
        );
        let events_before_inbox = target
            .list_control_room_events(0)
            .expect("events before inbox")
            .events
            .len();
        target
            .apply_federation_steward_assist_inbox(&inbox, now + 58)
            .expect("local inbox");
        let events_after_inbox = target
            .list_control_room_events(0)
            .expect("events after inbox")
            .events
            .len();
        assert_eq!(events_after_inbox, events_before_inbox + 1);
        target
            .apply_federation_steward_assist_inbox(&inbox, now + 58)
            .expect("unchanged inbox");
        assert_eq!(
            target
                .list_control_room_events(0)
                .expect("events after unchanged inbox")
                .events
                .len(),
            events_after_inbox,
            "unchanged polls must not churn the live feed"
        );
        let response = target
            .queue_federation_steward_assist_response(
                inbox.requests[0].id,
                FederationStewardAssistState::Accepted,
                now + 59,
            )
            .expect("queue response");
        let response_receipt = keeper
            .apply_federation_steward_assist_command(
                &target_acceptance.node_credential,
                &response.command,
                now + 60,
            )
            .expect("apply response");
        assert_eq!(
            response_receipt.outcome,
            FederationStewardAssistOutcome::Applied
        );
        assert_eq!(
            response_receipt
                .request
                .as_ref()
                .map(|request| request.state),
            Some(FederationStewardAssistState::Accepted)
        );
        target
            .apply_federation_steward_assist_receipt(&response_receipt, now + 61)
            .expect("response receipt");
        let refreshed = keeper
            .federation_steward_assist_inbox(&target_acceptance.node_credential, now + 62)
            .expect("refresh");
        target
            .apply_federation_steward_assist_inbox(&refreshed, now + 63)
            .expect("apply refresh");
        let local = target
            .federation_steward_assist_local_state()
            .expect("local state");
        assert_eq!(
            local.incoming[0].state,
            FederationStewardAssistState::Accepted
        );
        let source_view = keeper
            .federation_steward_assist_inbox(&steward_acceptance.node_credential, now + 64)
            .expect("source status poll");
        steward
            .apply_federation_steward_assist_inbox(&source_view, now + 65)
            .expect("source projection");
        let source_local = steward
            .federation_steward_assist_local_state()
            .expect("source state");
        assert_eq!(
            source_local.sent[0].state,
            FederationStewardAssistState::Accepted
        );

        let connection = target.connection().expect("connection");
        let injected = connection
            .query_row("SELECT COUNT(*) FROM decision_deliveries", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("delivery count");
        assert_eq!(
            injected, 0,
            "Assist remains a visible queue item, not a terminal injection"
        );
    }

    #[test]
    fn keeper_durably_rejects_assistance_after_scope_is_revoked() {
        let now = 500_000;
        let keeper = TaskStore::in_memory().expect("keeper");
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .expect("apiary");
        let (steward, acceptance) = join_member(&keeper, now + 10);
        let (_target, target_acceptance) = join_member(&keeper, now + 30);
        let identity = steward.local_hive_identity().expect("identity");
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let delegation = keeper
            .set_stewardship(
                identity.operator.id,
                &[target_hive_id],
                &[StewardCapability::Observe, StewardCapability::Assist],
                now + 50,
            )
            .expect("delegation");
        let snapshot = keeper
            .federation_stewardship_snapshot(&acceptance.node_credential, now + 51)
            .expect("scope");
        steward
            .apply_federation_stewardship_snapshot(&snapshot, now + 52)
            .expect("projection");
        let queued = steward
            .queue_federation_steward_assist(target_hive_id, "Can I help?", now + 53)
            .expect("queue");
        keeper
            .revoke_stewardship(delegation.id, now + 54)
            .expect("revoke");
        let receipt = keeper
            .apply_federation_steward_assist_command(
                &acceptance.node_credential,
                &queued.command,
                now + 55,
            )
            .expect("durable rejection");
        assert_eq!(receipt.outcome, FederationStewardAssistOutcome::Rejected);
        assert!(receipt.request.is_none());
    }
}
