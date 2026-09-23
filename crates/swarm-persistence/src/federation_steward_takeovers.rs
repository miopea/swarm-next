use std::str::FromStr;

use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, FederationStewardTakeoverAction, FederationStewardTakeoverCommand,
    FederationStewardTakeoverCommandId, FederationStewardTakeoverInbox,
    FederationStewardTakeoverLease, FederationStewardTakeoverLeaseId,
    FederationStewardTakeoverLocalState, FederationStewardTakeoverOutboxEntry,
    FederationStewardTakeoverOutboxState, FederationStewardTakeoverOutcome,
    FederationStewardTakeoverReceipt, FederationStewardTakeoverRelayAuthorization,
    FederationStewardTakeoverRelayRole, FederationStewardTakeoverState, HiveId, LocalApiaryContext,
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
pub const STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION: u16 = 9;
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

    /// Authenticates one exact active relay participant without exposing any
    /// private target-Hive or terminal identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the credential, lease, revision, current scope,
    /// membership, or active lifetime no longer authorizes relay traffic.
    pub fn authorize_federation_steward_takeover_relay(
        &self,
        node_credential: &str,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverRelayAuthorization, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        expire_open_leases(&transaction, member.apiary, now)?;
        let lease = lease_by_id(&transaction, lease_id)?
            .filter(|lease| {
                lease.apiary_id == member.apiary
                    && lease.state == FederationStewardTakeoverState::Active
                    && lease.revision == revision
                    && lease.expires_at > now
            })
            .ok_or(TaskStoreError::InvalidFederationStewardTakeover)?;
        let role = if member.hive == lease.target_hive_id {
            FederationStewardTakeoverRelayRole::Target
        } else if member.hive == lease.source_hive_id
            && member.operator == lease.source_operator_id
            // ⚠️ NOT `== lease.stewardship_id`. A Keeper-sourced lease carries
            // `None`, and comparing Options would have matched any member who
            // also has no stewardship — authorizing them onto Keeper's relay.
            // The hive and operator checks above already make that unreachable,
            // but this is the kind of safety that must not depend on a
            // coincidence two lines up.
            && member_holds_source_authority(&transaction, &member, &lease)?
        {
            FederationStewardTakeoverRelayRole::Source
        } else {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        };
        transaction.commit()?;
        Ok(FederationStewardTakeoverRelayAuthorization { lease, role })
    }

    /// Records a reconciliation debt for every local takeover that has ended.
    ///
    /// ⚠️ DERIVED FROM THE LEASE TABLE RATHER THAN HOOKED INTO EACH CLOSING
    /// PATH. A takeover ends by release, reclaim, expiry, revocation, departure
    /// or a restart, and a hook on each is six places to forget one. Reading
    /// the closed rows means a path added later is covered before anybody
    /// remembers it needs to be.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn record_owed_takeover_recovery(&self, now: i64) -> Result<(), TaskStoreError> {
        let identity = self.local_hive_identity()?;
        self.connection()?.execute(
            "INSERT OR IGNORE INTO local_takeover_recovery
                 (lease_id, target_hive_id, ended_at, reconciled_at)
             SELECT lease_id, target_hive_id, COALESCE(ended_at, ?2), NULL
             FROM local_federation_steward_takeover_leases
             WHERE target_hive_id = ?1 AND state NOT IN ('requested','active')",
            params![identity.hive.id.to_string(), now],
        )?;
        Ok(())
    }

    /// What this Hive still owes before Queen automation may resume.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn owed_takeover_recovery(
        &self,
    ) -> Result<Vec<(FederationStewardTakeoverLeaseId, u64)>, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        // The revision comes along because clearing the host's authority needs
        // the exact lease it was installed under; a debt you cannot act on is
        // just a pause with extra steps.
        let mut statement = connection.prepare(
            "SELECT recovery.lease_id, COALESCE(lease.revision, 0)
             FROM local_takeover_recovery recovery
             LEFT JOIN local_federation_steward_takeover_leases lease
               ON lease.lease_id = recovery.lease_id
             WHERE recovery.target_hive_id = ?1 AND recovery.reconciled_at IS NULL
             ORDER BY recovery.ended_at",
        )?;
        let rows = statement.query_map(params![identity.hive.id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?;
        let mut owed = Vec::new();
        for row in rows {
            let (id, revision) = row?;
            owed.push((parse_domain_id(&id)?, revision));
        }
        Ok(owed)
    }

    /// Records that this Hive has put its own terminal back in order.
    ///
    /// ⚠️ CALLED ONLY AFTER THE HOST AUTHORITY IS ACTUALLY GONE. Marking this
    /// on the strength of the lease row being closed would restore exactly the
    /// gap it exists to close — the row closing is the QUESTION, not the answer.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn complete_takeover_recovery(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "UPDATE local_takeover_recovery SET reconciled_at = ?2
             WHERE lease_id = ?1 AND reconciled_at IS NULL",
            params![lease_id.to_string(), now],
        )?;
        Ok(())
    }

    /// Reconciles this Hive's own takeover state after a restart.
    ///
    /// ⚠️ A DURABLE LEASE CAN OUTLIVE THE AUTHORITY THAT ENFORCES IT, and that
    /// asymmetry is the whole reason this exists. The pause on Queen automation
    /// is derived from the durable lease row, but the terminal authority that
    /// makes a takeover real lives in the terminal host's memory. Restart the
    /// host and the row survives while the authority does not: automation stays
    /// paused for a takeover that is no longer happening, and the Hive sits
    /// doing nothing on behalf of nobody.
    ///
    /// So a lease is kept only if something could still be controlled through
    /// it. Anything past its expiry, and anything naming this Hive as target
    /// while no Queen session is running, is ended as `Expired` — the lease did
    /// not survive, which is exactly what that state means.
    ///
    /// Returns the leases that DID survive and still name this Hive as target,
    /// so the caller can reinstall their terminal authority. An empty result
    /// means automation is free to resume.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn reconcile_local_takeovers(
        &self,
        now: i64,
    ) -> Result<Vec<FederationStewardTakeoverLease>, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE local_federation_steward_takeover_leases
             SET state = 'expired', ended_at = COALESCE(ended_at, ?1), synced_at = ?1
             WHERE state IN ('requested','active') AND expires_at <= ?1",
            params![now],
        )?;
        let queen_session_exists = transaction
            .query_row(
                "SELECT 1 FROM worker_profiles p
                 JOIN worker_sessions s ON s.worker_id = p.id AND s.ended_at IS NULL
                 WHERE p.role = 'queen' AND p.archived_at IS NULL LIMIT 1",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !queen_session_exists {
            // ⚠️ NOTHING TO TAKE OVER MEANS NO TAKEOVER. Keeping the lease would
            // hold this Hive's automation down waiting for a Queen that is not
            // running, which is indistinguishable from being wedged.
            transaction.execute(
                "UPDATE local_federation_steward_takeover_leases
                 SET state = 'expired', ended_at = COALESCE(ended_at, ?1), synced_at = ?1
                 WHERE state IN ('requested','active') AND target_hive_id = ?2",
                params![now, identity.hive.id.to_string()],
            )?;
        }
        // Anything this pass just ended owes a reconciliation before automation
        // may resume, and so does anything that ended while the process was down.
        transaction.execute(
            "INSERT OR IGNORE INTO local_takeover_recovery
                 (lease_id, target_hive_id, ended_at, reconciled_at)
             SELECT lease_id, target_hive_id, COALESCE(ended_at, ?2), NULL
             FROM local_federation_steward_takeover_leases
             WHERE target_hive_id = ?1 AND state NOT IN ('requested','active')",
            params![identity.hive.id.to_string(), now],
        )?;
        let survivors = read_leases(
            &transaction,
            "local_federation_steward_takeover_leases",
            "WHERE state = 'active' AND target_hive_id = ?1 AND expires_at > ?2
             ORDER BY requested_at",
            params![identity.hive.id.to_string(), now],
        )?;
        transaction.commit()?;
        Ok(survivors)
    }

    /// Who took over which Hive, when, why, and why it ended.
    ///
    /// ⚠️ AN AUDIT NOBODY CAN READ IS NOT AN AUDIT, which is why this is part of
    /// ADR 0036's release gate rather than a nice-to-have. Takeover is the one
    /// Apiary capability that lets someone type into another operator's
    /// machine; the record of who did that, and of the operator taking it back,
    /// is the thing that makes the capability accountable rather than merely
    /// powerful.
    ///
    /// Newest first, bounded. Keeper-side, because Keeper is the authority that
    /// granted every one of them.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn apiary_takeover_audit(
        &self,
        limit: u32,
    ) -> Result<Vec<swarm_domain::TakeoverAuditEntry>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let leases = read_leases(
            &transaction,
            "apiary_steward_takeover_leases",
            "ORDER BY requested_at DESC, lease_id DESC LIMIT ?1",
            params![limit.min(200)],
        )?;
        let mut entries = Vec::with_capacity(leases.len());
        for lease in leases {
            // The reclaim reason lives in the command that ended it, not on the
            // lease: the lease's own reason is why the takeover STARTED, and
            // collapsing the two would lose the operator's account of taking
            // their machine back.
            let reclaim_reason = transaction
                .query_row(
                    "SELECT command_json FROM apiary_steward_takeover_commands
                     WHERE lease_id = ?1 AND outcome = 'applied'
                     ORDER BY processed_at DESC",
                    params![lease.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .and_then(|json| {
                    serde_json::from_str::<FederationStewardTakeoverCommand>(&json).ok()
                })
                .and_then(|command| match command.action {
                    FederationStewardTakeoverAction::Reclaim { reason, .. } => Some(reason),
                    _ => None,
                });
            entries.push(swarm_domain::TakeoverAuditEntry {
                lease,
                reclaim_reason,
            });
        }
        transaction.commit()?;
        Ok(entries)
    }

    /// Keeper taking over a member Hive, on its own authority.
    ///
    /// ⚠️ KEEPER DOES NOT TRAVEL THE MEMBER COMMAND PATH. That path exists so a
    /// Steward's Hive can journal an intent before network I/O and retry it
    /// idempotently; Keeper IS the authority and writes to its own store, so
    /// routing it through an outbound queue to itself would be ceremony that
    /// could disagree with the table beside it.
    ///
    /// Everything the ADR requires of a Steward still holds: one bounded lease
    /// per target, a REASON (unlike watching, which the operator explicitly
    /// exempted), an active member target, and a conflict rather than a silent
    /// replacement when someone already holds the Hive.
    ///
    /// # Errors
    /// Refuses a caller that is not this Apiary's Keeper, an empty or oversized
    /// reason, a target that is not an active member, and a target somebody is
    /// already holding.
    pub fn open_keeper_takeover(
        &self,
        target_hive_id: HiveId,
        reason: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverLease, TaskStoreError> {
        if !valid_reason(reason) {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let apiary_id = keeper_apiary(&transaction, &identity)?;
        expire_open_leases(&transaction, apiary_id, now)?;
        if !active_member_hive(&transaction, apiary_id, target_hive_id)? {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        if open_lease_for_target(&transaction, apiary_id, target_hive_id)?.is_some() {
            return Err(TaskStoreError::FederationStewardTakeoverQueueFull);
        }
        let lease = FederationStewardTakeoverLease {
            id: FederationStewardTakeoverLeaseId::new(),
            apiary_id,
            source_hive_id: identity.hive.id,
            target_hive_id,
            source_operator_id: identity.operator.id,
            // Keeper's own authority. See `FederationStewardTakeoverLease`.
            stewardship_id: None,
            reason: reason.trim().to_owned(),
            state: FederationStewardTakeoverState::Requested,
            revision: 1,
            requested_at: now,
            acknowledged_at: None,
            expires_at: now.saturating_add(REQUEST_LIFETIME_SECONDS),
            ended_at: None,
        };
        insert_keeper_lease(&transaction, &lease)?;
        transaction.commit()?;
        Ok(lease)
    }

    /// Extends an active takeover because its SOURCE is typing into it.
    ///
    /// ⚠️ A STEWARD'S TAKEOVER ENDED FIVE MINUTES IN, MID-KEYSTROKE, AND NOTHING
    /// COULD STOP IT. Renewal-on-input existed only for a lease the Keeper holds
    /// itself; a Steward's keystrokes reach the Keeper through the federation
    /// relay, where the Keeper-only renewal refused the lease for naming a
    /// stewardship. The lease lapsed while the Steward was typing.
    ///
    /// Authority comes from the caller, not from here: this is called only from
    /// the relay's SOURCE side, whose connection was authorised as this lease's
    /// source — the Keeper's own browser by a single-use grant, a Steward's Hive
    /// by its node credential against this exact lease. That is the
    /// "authenticated input" ADR 0036 renews on. Only an ACTIVE, UNEXPIRED lease
    /// moves: input never revives a lease that was reclaimed, released or lapsed.
    ///
    /// # Errors
    /// Returns an error when persistence fails. `Ok(false)` means nothing was
    /// eligible, which is ordinary.
    pub fn extend_takeover_on_source_input(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let changed = self.connection()?.execute(
            "UPDATE apiary_steward_takeover_leases
             SET expires_at = ?2, revision = revision + 1, updated_at = ?1
             WHERE lease_id = ?3 AND state = 'active' AND expires_at > ?1",
            params![
                now,
                now.saturating_add(ACTIVE_LIFETIME_SECONDS),
                lease_id.to_string()
            ],
        )?;
        Ok(changed == 1)
    }

    /// Keeper extending or ending a takeover it holds.
    ///
    /// ⚠️ NOT REVISION-FENCED EITHER, for the same reason reclaim is not: the
    /// target acknowledging moves the revision, and Keeper would otherwise have
    /// to re-read before every renewal to keep holding a lease it already owns.
    /// The exclusivity that matters is enforced by `one_open_takeover_per_target`
    /// and by requiring Keeper to be the recorded source.
    ///
    /// # Errors
    /// Refuses a caller that is not this Apiary's Keeper, and a lease Keeper
    /// does not hold or that is no longer open.
    pub fn transition_keeper_takeover(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        to: FederationStewardTakeoverState,
        now: i64,
    ) -> Result<FederationStewardTakeoverLease, TaskStoreError> {
        if !matches!(
            to,
            FederationStewardTakeoverState::Active | FederationStewardTakeoverState::Released
        ) {
            return Err(TaskStoreError::InvalidFederationStewardTakeover);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let apiary_id = keeper_apiary(&transaction, &identity)?;
        expire_open_leases(&transaction, apiary_id, now)?;
        let lease = lease_by_id(&transaction, lease_id)?
            .filter(|lease| {
                lease.apiary_id == apiary_id
                    && lease.source_hive_id == identity.hive.id
                    && lease.source_operator_id == identity.operator.id
                    && lease.stewardship_id.is_none()
                    // ⚠️ RELEASING ACCEPTS ANY OPEN LEASE, NOT ONLY AN ACTIVE
                    // ONE. Requiring Active meant a request the target had not
                    // taken up yet could not be withdrawn — and a Keeper that
                    // closes the window before acknowledgement left the target
                    // holding a `Requested` lease that nothing could end,
                    // blocking every later takeover of that Hive with
                    // "could not be taken over". Extending still requires a
                    // lease that is actually Active, because there is nothing
                    // to extend otherwise.
                    && if to == FederationStewardTakeoverState::Released {
                        lease.state.is_open()
                    } else {
                        lease.state == FederationStewardTakeoverState::Active
                    }
            })
            .ok_or(TaskStoreError::InvalidFederationStewardTakeover)?;
        let expires_at = if to == FederationStewardTakeoverState::Active {
            now.saturating_add(ACTIVE_LIFETIME_SECONDS)
        } else {
            lease.expires_at
        };
        let ended_at = (!to.is_open()).then_some(now);
        let updated = FederationStewardTakeoverLease {
            state: to,
            revision: lease.revision.saturating_add(1),
            expires_at,
            ended_at,
            ..lease
        };
        transaction.execute(
            "UPDATE apiary_steward_takeover_leases
             SET state = ?1, revision = ?2, expires_at = ?3, ended_at = ?4, updated_at = ?5
             WHERE lease_id = ?6",
            params![
                updated.state.to_string(),
                updated.revision,
                updated.expires_at,
                updated.ended_at,
                now,
                updated.id.to_string()
            ],
        )?;
        transaction.commit()?;
        Ok(updated)
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
            Some(*expected_revision),
            FederationStewardTakeoverState::Requested,
        )?,
        FederationStewardTakeoverAction::Reclaim { lease_id, .. } => {
            // ⚠️ UNFENCED HERE TOO, for the same reason and one more: the local
            // projection only advances when this Hive next polls Keeper, so
            // straight after acknowledging, the revision on screen is already
            // behind. Refusing to even JOURNAL the reclaim would make the local
            // operator wait for a round trip to take their own machine back.
            require_local_lease(
                transaction,
                *lease_id,
                identity.hive.id,
                false,
                None,
                FederationStewardTakeoverState::Active,
            )?;
            transaction.execute(
                "UPDATE local_federation_steward_takeover_leases
                 SET state = 'reclaimed', revision = revision + 1,
                     ended_at = ?1, synced_at = ?1
                 WHERE lease_id = ?2 AND state = 'active'",
                params![now, lease_id.to_string()],
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
            Some(*expected_revision),
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
            // A command arriving on a member credential is always a Steward's.
            // Keeper does not travel this path — it acts on its own store — so
            // a missing stewardship here is still a refusal rather than Keeper
            // authority.
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
                stewardship_id: Some(stewardship_id),
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

/// Whether this member credential still carries the authority the lease was
/// granted under.
///
/// ⚠️ A KEEPER-SOURCED LEASE ANSWERS `false` HERE, ALWAYS AND ON PURPOSE. Its
/// `stewardship_id` is `None`, and comparing `None` to "this member has no
/// stewardship" would read as a match.
///
/// ⚠️ THIS GUARD IS UNREACHABLE TODAY AND IS KEPT ANYWAY — stated plainly
/// because an ablation proved it. Removing the `None` arm and comparing the two
/// Options directly leaves every takeover test passing, because `actor_allowed`
/// independently requires the caller's hive and operator to match the lease's
/// source, and Keeper's hive is never a member. So nothing here PINS this; it is
/// defence in depth against a future refactor of `actor_allowed`, and claiming
/// a test covers it would be exactly the kind of false assurance this file is
/// careful about elsewhere.
fn member_holds_source_authority(
    transaction: &Transaction<'_>,
    member: &MemberCredentialContext,
    lease: &FederationStewardTakeoverLease,
) -> Result<bool, TaskStoreError> {
    let Some(granted) = lease.stewardship_id else {
        return Ok(false);
    };
    Ok(authorized_stewardship(
        transaction,
        member.apiary,
        member.operator,
        lease.target_hive_id,
    )? == Some(granted))
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
    // ⚠️ RECLAIM IS NOT REVISION-FENCED, AND THAT IS THE OPERATOR'S RULING
    // RATHER THAN A RELAXATION. Every renewal bumps the revision, and a Steward
    // actively working renews constantly — so fencing reclaim means the busier
    // the remote actor is, the more reliably the person at the keyboard is told
    // "no". Measured in `reclaim_is_not_fenced_by_a_revision_the_steward_keeps_moving`:
    // one ordinary renewal between projection and reclaim was enough to refuse
    // the local operator. The alternatives — an unreclaimable lease and a
    // Keeper-settable lock — were offered on 2026-09-21 and declined.
    //
    // Nothing else is relaxed: the actor must still be the target Hive, and the
    // lease must still be Active.
    let fence_revision = to != FederationStewardTakeoverState::Reclaimed;
    if !actor_allowed
        || lease.state != from
        || (fence_revision && lease.revision != expected_revision)
        || lease.expires_at <= now
        || (source_action && !member_holds_source_authority(transaction, member, &lease)?)
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

/// The Apiary this Hive keeps, refusing any caller that is not its Keeper.
fn keeper_apiary(
    transaction: &Transaction<'_>,
    identity: &swarm_domain::HiveIdentity,
) -> Result<swarm_domain::ApiaryId, TaskStoreError> {
    transaction
        .query_row(
            "SELECT id FROM apiaries WHERE keeper_operator_id = ?1 AND collapsed_at IS NULL",
            params![identity.operator.id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(TaskStoreError::ApiaryKeeperRequired)
        .and_then(|id| parse_domain_id(&id).map_err(Into::into))
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
            lease.stewardship_id.map(|id| id.to_string()),
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
            lease.stewardship_id.map(|id| id.to_string()),
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
        stewardship_id: row
            .get::<_, Option<String>>(5)?
            .map(|id| parse_domain_id(&id))
            .transpose()?,
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

/// `expected_revision` of `None` means "whatever revision this Hive holds".
///
/// Used only by reclaim, where fencing on a revision the local operator cannot
/// keep up with is the bug rather than the safety.
fn require_local_lease(
    transaction: &Transaction<'_>,
    lease_id: FederationStewardTakeoverLeaseId,
    local_hive_id: HiveId,
    source: bool,
    expected_revision: Option<u64>,
    expected_state: FederationStewardTakeoverState,
) -> Result<(), TaskStoreError> {
    let found = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_federation_steward_takeover_leases
         WHERE lease_id = ?1 AND (?2 IS NULL OR revision = ?2) AND state = ?5
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

/// Automation resumes only after the Hive has reconciled locally.
///
/// ⚠️ THE GAP THIS CLOSES WAS MEASURED, NOT SUSPECTED. Before this, Queen
/// automation resumed the instant the lease row closed — so a Steward could
/// leave a half-typed command in the terminal, release, and have Queen inject
/// into it on the next tick. ADR 0036 says every ending "resumes normal
/// automation only after local reconciliation"; nothing implemented that.
///
/// One row per ended takeover, cleared when this Hive has confirmed the
/// terminal authority is gone. Durable because the thing it guards is durable:
/// a process restart must not be a way to skip the reconciliation.
pub(super) fn migrate_takeover_recovery(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_takeover_recovery (
             lease_id TEXT PRIMARY KEY,
             target_hive_id TEXT NOT NULL,
             ended_at INTEGER NOT NULL,
             reconciled_at INTEGER
         );
         CREATE INDEX IF NOT EXISTS local_takeover_recovery_owed
             ON local_takeover_recovery(target_hive_id) WHERE reconciled_at IS NULL;",
    )?;
    // ⚠️ EVERY LEASE THAT ALREADY ENDED COUNTS AS RECONCILED. They closed before
    // this gate existed, so their terminals were put back long ago — and a Hive
    // that upgraded into an automation pause it could never clear would be
    // exactly the wedge this whole item is about.
    transaction.execute(
        "INSERT OR IGNORE INTO local_takeover_recovery
             (lease_id, target_hive_id, ended_at, reconciled_at)
         SELECT lease_id, target_hive_id, COALESCE(ended_at, 0), COALESCE(ended_at, 0)
         FROM local_federation_steward_takeover_leases
         WHERE state NOT IN ('requested','active')",
        [],
    )?;
    transaction.pragma_update(None, "user_version", crate::TAKEOVER_RECOVERY_SCHEMA_MARKER)
}

/// Keeper may take over, so a lease need not name a stewardship.
///
/// ⚠️ A TABLE REBUILD, because `SQLite` cannot drop a `NOT NULL`. Both lease tables
/// are rebuilt rather than only the Keeper-side one: the member's local
/// projection mirrors the same rows, and a member that could not store a
/// Keeper-sourced lease would fail to project the takeover being done to it —
/// which is the one thing it must always be able to show its operator.
pub(super) fn migrate_keeper_takeover_authority(
    transaction: &Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE apiary_steward_takeover_leases_rebuilt (
             lease_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             source_hive_id TEXT NOT NULL REFERENCES hives(id), target_hive_id TEXT NOT NULL REFERENCES hives(id),
             source_operator_id TEXT NOT NULL REFERENCES operators(id), stewardship_id TEXT REFERENCES stewardships(id),
             reason TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('requested','active','released','reclaimed','expired')),
             revision INTEGER NOT NULL, requested_at INTEGER NOT NULL, acknowledged_at INTEGER,
             expires_at INTEGER NOT NULL, ended_at INTEGER, updated_at INTEGER NOT NULL
         );
         INSERT INTO apiary_steward_takeover_leases_rebuilt
             SELECT lease_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
                    stewardship_id, reason, state, revision, requested_at, acknowledged_at,
                    expires_at, ended_at, updated_at
             FROM apiary_steward_takeover_leases;
         DROP TABLE apiary_steward_takeover_leases;
         ALTER TABLE apiary_steward_takeover_leases_rebuilt
             RENAME TO apiary_steward_takeover_leases;
         CREATE UNIQUE INDEX IF NOT EXISTS one_open_takeover_per_target
             ON apiary_steward_takeover_leases(apiary_id, target_hive_id)
             WHERE state IN ('requested','active');
         CREATE INDEX IF NOT EXISTS apiary_steward_takeover_participants
             ON apiary_steward_takeover_leases(apiary_id, source_hive_id, target_hive_id, requested_at DESC);
         CREATE TABLE local_federation_steward_takeover_leases_rebuilt (
             lease_id TEXT PRIMARY KEY, apiary_id TEXT NOT NULL, source_hive_id TEXT NOT NULL,
             target_hive_id TEXT NOT NULL, source_operator_id TEXT NOT NULL, stewardship_id TEXT,
             reason TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('requested','active','released','reclaimed','expired')),
             revision INTEGER NOT NULL, requested_at INTEGER NOT NULL, acknowledged_at INTEGER,
             expires_at INTEGER NOT NULL, ended_at INTEGER, synced_at INTEGER NOT NULL
         );
         INSERT INTO local_federation_steward_takeover_leases_rebuilt
             SELECT lease_id, apiary_id, source_hive_id, target_hive_id, source_operator_id,
                    stewardship_id, reason, state, revision, requested_at, acknowledged_at,
                    expires_at, ended_at, synced_at
             FROM local_federation_steward_takeover_leases;
         DROP TABLE local_federation_steward_takeover_leases;
         ALTER TABLE local_federation_steward_takeover_leases_rebuilt
             RENAME TO local_federation_steward_takeover_leases;",
    )?;
    transaction.pragma_update(None, "user_version", crate::KEEPER_TAKEOVER_SCHEMA_MARKER)
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

    /// Drives one takeover to Active, with BOTH sides projecting it, and
    /// returns the active receipt.
    ///
    /// Extracted because the path to an active lease is eight round trips and
    /// says nothing about what any individual test is checking.
    fn activate_takeover(
        keeper: &TaskStore,
        steward: &TaskStore,
        steward_acceptance: &FederationJoinAcceptance,
        target: &TaskStore,
        target_acceptance: &FederationJoinAcceptance,
        now: i64,
    ) -> FederationStewardTakeoverReceipt {
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let request = steward
            .queue_federation_steward_takeover(target_hive_id, "Release is blocked.", now)
            .expect("journal request");
        keeper
            .apply_federation_steward_takeover_command(
                &steward_acceptance.node_credential,
                &request.command,
                now + 1,
            )
            .expect("request");
        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 2)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        target
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .expect("bind queen");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 3)
            .expect("target projection");
        let lease = inbox.leases.first().expect("requested lease");
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(lease.id, lease.revision, now + 4)
            .expect("journal acknowledgement");
        let active = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 5,
            )
            .expect("acknowledge");
        target
            .apply_federation_steward_takeover_receipt(&active, now + 5)
            .expect("target receipt");
        // ⚠️ THE TARGET HAS TO POLL TO SEE ITS OWN ACKNOWLEDGEMENT LAND.
        // `apply_..._receipt` advances the OUTBOX only; the lease projection
        // moves on the next inbox. So a local surface cannot act on the active
        // lease until a round trip completes — one of two reasons "immediately"
        // was not quite what the word meant.
        for (store, credential) in [
            (target, &target_acceptance.node_credential),
            (steward, &steward_acceptance.node_credential),
        ] {
            let inbox = keeper
                .federation_steward_takeover_inbox(credential, now + 6)
                .expect("poll");
            store
                .apply_federation_steward_takeover_inbox(&inbox, now + 6)
                .expect("projection");
        }
        active
    }

    /// ⚠️ THE AUDIT MUST CARRY BOTH REASONS, AND THEY ARE DIFFERENT CLAIMS. The
    /// lease's reason is why someone took the Hive; the reclaim reason is the
    /// local operator's account of taking it back. Collapsing them would lose
    /// the half that matters most when the question is whether a takeover
    /// should have happened at all.
    #[test]
    fn the_audit_records_who_took_over_why_and_why_it_ended() {
        let now = 700_000;
        let (keeper, _steward, _steward_acceptance, target, target_acceptance) =
            setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let lease = keeper
            .open_keeper_takeover(target_hive_id, "Incident: operator unreachable.", now + 53)
            .expect("keeper takeover");
        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 54)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        target
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .expect("bind");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 55)
            .expect("projection");
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(
                lease.id,
                inbox.leases[0].revision,
                now + 56,
            )
            .expect("journal acknowledgement");
        keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 57,
            )
            .expect("acknowledge");

        // Mid-takeover, the audit already names who holds the Hive and why.
        let live = keeper.apiary_takeover_audit(10).expect("audit");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].lease.reason, "Incident: operator unreachable.");
        assert_eq!(
            live[0].lease.stewardship_id, None,
            "Keeper's own authority is legible in the record"
        );
        assert_eq!(live[0].lease.state, FederationStewardTakeoverState::Active);
        assert_eq!(
            live[0].reclaim_reason, None,
            "nothing has been reclaimed yet"
        );

        // The person at the machine takes it back, and says why.
        let refreshed = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 58)
            .expect("poll");
        target
            .apply_federation_steward_takeover_inbox(&refreshed, now + 58)
            .expect("projection");
        let projected = target
            .federation_steward_takeover_local_state()
            .expect("local")
            .leases
            .into_iter()
            .find(|held| held.id == lease.id)
            .expect("projected");
        let reclaim = target
            .queue_federation_steward_takeover_reclaim(
                lease.id,
                projected.revision,
                "Mid-deploy; taking my machine back.",
                now + 59,
            )
            .expect("journal reclaim");
        keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &reclaim.command,
                now + 60,
            )
            .expect("reclaim");

        let audit = keeper.apiary_takeover_audit(10).expect("audit");
        assert_eq!(audit.len(), 1);
        assert_eq!(
            audit[0].lease.state,
            FederationStewardTakeoverState::Reclaimed
        );
        assert_eq!(
            audit[0].lease.reason, "Incident: operator unreachable.",
            "why it started"
        );
        assert_eq!(
            audit[0].reclaim_reason.as_deref(),
            Some("Mid-deploy; taking my machine back."),
            "and why it ended, in the operator's own words"
        );
    }

    /// ⚠️ EVERY RECLAIM CARRIES A REASON BECAUSE THE BOUNDARY REFUSES ONE
    /// WITHOUT. The operator's ruling is that reclaim is always available and
    /// always audited; "always audited" is only true if a blank reason cannot
    /// get through, so this pins the refusal rather than trusting the caller.
    #[test]
    fn a_reclaim_without_a_reason_is_refused_rather_than_recorded_blank() {
        let now = 700_000;
        let (_keeper, _steward, _steward_acceptance, target, _target_acceptance) =
            setup_takeover(now);
        let lease_id = FederationStewardTakeoverLeaseId::new();
        for blank in ["", "   ", "\n\t"] {
            assert!(
                target
                    .queue_federation_steward_takeover_reclaim(lease_id, 2, blank, now + 60)
                    .is_err(),
                "a reclaim with no account of itself is not recorded"
            );
        }
    }

    /// ⚠️ A RELEASED TAKEOVER MUST NOT HAND THE TERMINAL STRAIGHT TO QUEEN.
    ///
    /// ADR 0036: every ending "resumes normal automation ONLY AFTER local
    /// reconciliation". Measured before this was built: automation resumed the
    /// instant the lease row closed, so a Steward could leave a half-typed
    /// command in the terminal, release, and have Queen inject into it on the
    /// next tick. The row closing was the resume.
    #[test]
    fn automation_stays_paused_until_the_hive_has_reconciled_locally() {
        let now = 700_000;
        let (keeper, _steward, _steward_acceptance, target, target_acceptance) =
            setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let lease = keeper
            .open_keeper_takeover(target_hive_id, "Incident.", now + 53)
            .expect("keeper takeover");
        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 54)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        target
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .expect("bind");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 55)
            .expect("projection");
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(
                lease.id,
                inbox.leases[0].revision,
                now + 56,
            )
            .expect("journal acknowledgement");
        keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 57,
            )
            .expect("acknowledge");
        let active = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 58)
            .expect("poll");
        target
            .apply_federation_steward_takeover_inbox(&active, now + 58)
            .expect("projection");
        assert!(
            !target
                .worker_accepts_injection(queen.id, now + 58)
                .expect("guard"),
            "paused during the takeover"
        );

        // Keeper releases. The target learns on its next poll.
        keeper
            .transition_keeper_takeover(
                lease.id,
                FederationStewardTakeoverState::Released,
                now + 59,
            )
            .expect("release");
        let after = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 60)
            .expect("poll");
        target
            .apply_federation_steward_takeover_inbox(&after, now + 60)
            .expect("projection");

        target
            .record_owed_takeover_recovery(now + 60)
            .expect("record what is owed");
        assert_eq!(
            target
                .owed_takeover_recovery()
                .expect("owed")
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            vec![lease.id],
            "the ended takeover is owed a reconciliation"
        );
        assert!(
            !target
                .worker_accepts_injection(queen.id, now + 60)
                .expect("guard"),
            "the row closing is the QUESTION, not the answer"
        );

        // Only once this Hive has actually put its terminal back in order.
        target
            .complete_takeover_recovery(lease.id, now + 61)
            .expect("reconciled");
        assert!(
            target.owed_takeover_recovery().expect("owed").is_empty(),
            "nothing is owed once it is reconciled"
        );
        assert!(
            target
                .worker_accepts_injection(queen.id, now + 61)
                .expect("guard"),
            "and only then does automation resume"
        );
    }

    /// ⚠️ A RESTART MUST NOT BE A WAY TO SKIP THE RECONCILIATION. The debt is
    /// durable precisely because the process is not: a Hive that crashed while
    /// a Steward held its terminal comes back owing the same reconciliation it
    /// owed before, rather than waking up free.
    #[test]
    fn a_restart_does_not_clear_what_reconciliation_is_owed() {
        let now = 700_000;
        let (keeper, _steward, _steward_acceptance, target, target_acceptance) =
            setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let lease = keeper
            .open_keeper_takeover(target_hive_id, "Incident.", now + 53)
            .expect("keeper takeover");
        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 54)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        target
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .expect("bind");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 55)
            .expect("projection");

        // The lease lapses while nobody is looking, which a restart discovers.
        let after = now + 55 + REQUEST_LIFETIME_SECONDS + 1;
        let survivors = target.reconcile_local_takeovers(after).expect("reconcile");
        assert!(survivors.is_empty());
        assert_eq!(
            target
                .owed_takeover_recovery()
                .expect("owed")
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            vec![lease.id],
            "reconciliation is owed for a takeover that ended while the Hive was down"
        );
        assert!(
            !target
                .worker_accepts_injection(queen.id, after)
                .expect("guard"),
            "so automation does not simply resume on boot"
        );
    }

    /// ⚠️ A RESTART MUST NOT LEAVE A HIVE PAUSED FOR A TAKEOVER THAT IS NOT
    /// HAPPENING. The pause on Queen automation is derived from the durable
    /// lease row; the authority that makes a takeover real lives in the
    /// terminal host's memory. Restart the host and the row outlives the
    /// authority, so the Hive sits doing nothing on behalf of nobody — which
    /// from the outside is indistinguishable from being wedged, and this Hive
    /// has already lost a day to one of those.
    #[test]
    fn a_restart_ends_a_takeover_that_has_nothing_left_to_control() {
        let now = 700_000;
        let (keeper, _steward, _steward_acceptance, target, target_acceptance) =
            setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let lease = keeper
            .open_keeper_takeover(target_hive_id, "Incident.", now + 53)
            .expect("keeper takeover");
        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 54)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        let session = swarm_domain::WorkerSessionId::new();
        target.bind_worker_session(queen.id, session).expect("bind");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 55)
            .expect("target projection");
        assert!(
            !target
                .worker_accepts_injection(queen.id, now + 55)
                .expect("automation guard"),
            "automation is paused while the takeover stands"
        );

        // A restart with the Queen session still running keeps the takeover,
        // and hands it back so its terminal authority can be reinstalled.
        let survivors = target
            .reconcile_local_takeovers(now + 56)
            .expect("reconcile");
        assert!(
            survivors.is_empty(),
            "a requested lease is not yet active, so there is nothing to reinstall"
        );

        // Acknowledge, so the lease is genuinely active.
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(
                lease.id,
                inbox.leases[0].revision,
                now + 57,
            )
            .expect("journal acknowledgement");
        let active = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 58,
            )
            .expect("acknowledge");
        let refreshed = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 59)
            .expect("target poll");
        target
            .apply_federation_steward_takeover_inbox(&refreshed, now + 59)
            .expect("projection");
        let survivors = target
            .reconcile_local_takeovers(now + 60)
            .expect("reconcile");
        assert_eq!(
            survivors.len(),
            1,
            "an active takeover with a live Queen survives a restart"
        );
        assert_eq!(survivors[0].id, active.lease.as_ref().unwrap().id);

        // ⚠️ THE HOST RESTARTED: the Queen session is gone, so nothing can be
        // controlled through this lease any more.
        target
            .release_worker_session(session)
            .expect("session ended");
        let survivors = target
            .reconcile_local_takeovers(now + 61)
            .expect("reconcile");
        assert!(
            survivors.is_empty(),
            "nothing survives with no Queen to control"
        );
        // ⚠️ AND STILL DOES NOT RESUME YET. The lease is over, but ADR 0036
        // resumes automation only after LOCAL RECONCILIATION — so ending the
        // lease removes the takeover and settling the debt removes the pause.
        // This assertion changed when that gate was built: it used to say
        // automation resumed right here, which was precisely the gap.
        assert!(
            !target
                .worker_accepts_injection(queen.id, now + 61)
                .expect("automation guard"),
            "the lease ending is not by itself the resume"
        );
        for (owed, _) in target.owed_takeover_recovery().expect("owed") {
            target
                .complete_takeover_recovery(owed, now + 62)
                .expect("reconciled");
        }
        assert!(
            target
                .worker_accepts_injection(queen.id, now + 62)
                .expect("automation guard"),
            "and once reconciled it resumes rather than waiting out the lease"
        );
    }

    /// An expired lease is ended by reconciliation rather than left to be
    /// noticed, so a restart never resumes into a stale pause.
    #[test]
    fn a_restart_expires_a_lease_whose_time_had_already_run_out() {
        let now = 700_000;
        let (keeper, _steward, _steward_acceptance, target, target_acceptance) =
            setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        keeper
            .open_keeper_takeover(target_hive_id, "Incident.", now + 53)
            .expect("keeper takeover");
        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 54)
            .expect("target poll");
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        target
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .expect("bind");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 55)
            .expect("projection");

        let after_expiry = now + 55 + REQUEST_LIFETIME_SECONDS + 1;
        let survivors = target
            .reconcile_local_takeovers(after_expiry)
            .expect("reconcile");
        assert!(survivors.is_empty());
        let local = target
            .federation_steward_takeover_local_state()
            .expect("local state");
        assert_eq!(
            local.leases[0].state,
            FederationStewardTakeoverState::Expired,
            "the lease is ended in the record, not merely ignored by a query"
        );
        assert_eq!(local.leases[0].ended_at, Some(after_expiry));
    }

    /// ⚠️ KEEPER TAKES OVER ON THE SAME TERMS AS A STEWARD, which the
    /// 2026-09-21 interview settled. Keeper holds no stewardship over its own
    /// Apiary, so the lease records `None` — the absence IS the authority,
    /// exactly as `WatchAuthority::Keeper` works for watching.
    ///
    /// The target's side is unchanged: it still has to acknowledge before
    /// anything is active, and it can still reclaim.
    #[test]
    fn keeper_takes_over_on_its_own_authority_and_the_target_still_acknowledges() {
        let now = 700_000;
        let (keeper, _steward, _steward_acceptance, target, target_acceptance) =
            setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;

        let lease = keeper
            .open_keeper_takeover(
                target_hive_id,
                "Incident: the operator is unreachable.",
                now + 53,
            )
            .expect("keeper takeover");
        assert_eq!(lease.state, FederationStewardTakeoverState::Requested);
        assert_eq!(
            lease.stewardship_id, None,
            "Keeper's own authority, not a stewardship it does not hold"
        );

        // ⚠️ A REQUESTED LEASE GRANTS NOTHING, including to Keeper.
        assert!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &target_acceptance.node_credential,
                    lease.id,
                    lease.revision,
                    now + 54,
                )
                .is_err(),
            "no relay before the target has acknowledged"
        );

        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 55)
            .expect("target poll");
        assert_eq!(
            inbox.leases.len(),
            1,
            "the target is told who is taking over"
        );
        assert_eq!(inbox.leases[0].stewardship_id, None);
        let queen = target.ensure_queen("/workspace/queen").expect("queen");
        target
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .expect("bind queen");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 56)
            .expect("target projection");
        assert!(
            !target
                .worker_accepts_injection(queen.id, now + 56)
                .expect("automation guard"),
            "a Keeper takeover pauses competing Queen automation like any other"
        );

        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(
                lease.id,
                inbox.leases[0].revision,
                now + 57,
            )
            .expect("journal acknowledgement");
        let active = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 58,
            )
            .expect("acknowledge");
        assert_eq!(active.outcome, FederationStewardTakeoverOutcome::Applied);
        assert_eq!(
            active.lease.as_ref().map(|lease| lease.state),
            Some(FederationStewardTakeoverState::Active)
        );

        // Keeper renews and releases on its own store, without a command queue.
        let renewed = keeper
            .transition_keeper_takeover(lease.id, FederationStewardTakeoverState::Active, now + 59)
            .expect("keeper renew");
        assert!(renewed.expires_at > now + 59);
        let released = keeper
            .transition_keeper_takeover(
                lease.id,
                FederationStewardTakeoverState::Released,
                now + 60,
            )
            .expect("keeper release");
        assert_eq!(released.state, FederationStewardTakeoverState::Released);
        assert_eq!(released.ended_at, Some(now + 60));
    }

    /// A Steward holding the very same scope cannot drive a lease Keeper holds.
    ///
    /// ⚠️ WHAT THIS DOES AND DOES NOT PROVE. It pins the refusal, which is worth
    /// having. It does NOT pin the `None`-arm in `member_holds_source_authority`:
    /// ablating that guard leaves this test green, because the hive and operator
    /// checks refuse first. Said out loud because a test whose name suggests it
    /// guards something it does not is worse than no test — the refusal here is
    /// real, its cause is elsewhere.
    #[test]
    fn a_keeper_lease_is_not_drivable_through_a_member_credential() {
        let now = 700_000;
        let (keeper, steward, steward_acceptance, target, target_acceptance) = setup_takeover(now);
        let target_hive_id = target_acceptance.receipt.payload.member_hive_id;
        let lease = keeper
            .open_keeper_takeover(target_hive_id, "Incident.", now + 53)
            .expect("keeper takeover");

        // The Steward has a real stewardship over this very Hive, and still
        // cannot touch a lease Keeper holds.
        assert!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &steward_acceptance.node_credential,
                    lease.id,
                    lease.revision,
                    now + 54,
                )
                .is_err(),
            "a Steward does not inherit Keeper's lease by holding the same scope"
        );
        assert!(
            steward
                .queue_federation_steward_takeover_renewal(lease.id, lease.revision, now + 55)
                .is_err(),
            "and cannot even journal a renewal of it"
        );
        let _ = target;
    }

    /// ⚠️ RECLAIM MUST NOT LOSE A RACE IT IS THE WHOLE POINT OF WINNING.
    ///
    /// The operator's ruling, 2026-09-21: "The local operator reclaims from any
    /// authenticated local surface, IMMEDIATELY." The alternatives — an
    /// unreclaimable lease, and a Keeper-settable lock — were offered and
    /// declined, because a remote actor holding a machine against the person
    /// sitting at it leaves the audit trail as the only protection.
    ///
    /// A revision fence on reclaim quietly reintroduces exactly that. The
    /// Steward renews while working; each renewal bumps the revision; the local
    /// operator's reclaim carries whatever revision their screen last showed.
    /// The busier the Steward, the more reliably the person at the keyboard is
    /// told "no" — and they are mid-deploy or mid-incident, which is why they
    /// reached for it.
    #[test]
    fn reclaim_is_not_fenced_by_a_revision_the_steward_keeps_moving() {
        let now = 700_000;
        let (keeper, steward, steward_acceptance, target, target_acceptance) = setup_takeover(now);
        let active = activate_takeover(
            &keeper,
            &steward,
            &steward_acceptance,
            &target,
            &target_acceptance,
            now + 53,
        );
        let active_lease = active.lease.as_ref().expect("active lease");

        // The Steward keeps working; renewal is how an active lease stays alive.
        let renewal = steward
            .queue_federation_steward_takeover_renewal(
                active_lease.id,
                active_lease.revision,
                now + 61,
            )
            .expect("journal renewal");
        let renewed = keeper
            .apply_federation_steward_takeover_command(
                &steward_acceptance.node_credential,
                &renewal.command,
                now + 62,
            )
            .expect("renew");
        assert_eq!(renewed.outcome, FederationStewardTakeoverOutcome::Applied);
        let renewed_revision = renewed.lease.as_ref().unwrap().revision;

        // The person at the machine reclaims, carrying the revision their own
        // Hive last projected — all a local surface can ever send.
        let projected = target
            .federation_steward_takeover_local_state()
            .expect("local state")
            .leases
            .into_iter()
            .find(|lease| lease.id == active_lease.id)
            .expect("the target projects its own lease");
        assert_eq!(projected.state, FederationStewardTakeoverState::Active);
        assert!(
            renewed_revision > projected.revision,
            "the renewal moved the revision out from under the local operator"
        );
        let reclaim = target
            .queue_federation_steward_takeover_reclaim(
                active_lease.id,
                projected.revision,
                "Mid-deploy; taking my machine back.",
                now + 63,
            )
            .expect("journal reclaim");
        let outcome = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &reclaim.command,
                now + 64,
            )
            .expect("reclaim");
        assert_eq!(
            outcome.outcome,
            FederationStewardTakeoverOutcome::Applied,
            "the person at the keyboard wins, whatever the Steward did meanwhile"
        );
        assert_eq!(
            outcome.lease.as_ref().map(|lease| lease.state),
            Some(FederationStewardTakeoverState::Reclaimed)
        );
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
        assert!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &steward_acceptance.node_credential,
                    requested.lease.as_ref().unwrap().id,
                    requested.lease.as_ref().unwrap().revision,
                    now + 54,
                )
                .is_err()
        );
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
        assert_eq!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &steward_acceptance.node_credential,
                    active_lease.id,
                    active_lease.revision,
                    now + 60,
                )
                .expect("source relay authority")
                .role,
            FederationStewardTakeoverRelayRole::Source
        );
        assert_eq!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &target_acceptance.node_credential,
                    active_lease.id,
                    active_lease.revision,
                    now + 60,
                )
                .expect("target relay authority")
                .role,
            FederationStewardTakeoverRelayRole::Target
        );
        assert!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &steward_acceptance.node_credential,
                    active_lease.id,
                    active_lease.revision.saturating_sub(1),
                    now + 60,
                )
                .is_err()
        );
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

    /// ⚠️ A STEWARD'S TAKEOVER USED TO END FIVE MINUTES IN, MID-KEYSTROKE. The
    /// only renewal on input was for a lease the Keeper held itself, and a
    /// Steward's lease names a stewardship, so it was refused and lapsed while
    /// the Steward typed.
    #[test]
    fn a_steward_typing_into_a_takeover_keeps_it_alive_and_nothing_revives_one() {
        let now = 910_000;
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
        let lease_id = requested.lease.as_ref().unwrap().id;
        // Still only REQUESTED: input must not activate or extend it.
        assert!(
            !keeper
                .extend_takeover_on_source_input(lease_id, now + 54)
                .unwrap()
        );

        let inbox = keeper
            .federation_steward_takeover_inbox(&target_acceptance.node_credential, now + 55)
            .expect("target inbox");
        target
            .apply_federation_steward_takeover_inbox(&inbox, now + 56)
            .expect("projection");
        let acknowledgement = target
            .queue_federation_steward_takeover_acknowledgement(lease_id, 1, now + 57)
            .expect("ack");
        let active = keeper
            .apply_federation_steward_takeover_command(
                &target_acceptance.node_credential,
                &acknowledgement.command,
                now + 58,
            )
            .expect("active");
        let before = active.lease.as_ref().unwrap().expires_at;
        assert!(
            active.lease.as_ref().unwrap().stewardship_id.is_some(),
            "a Steward's lease"
        );

        // Four minutes in, the Steward is still typing.
        let typing_at = before - 60;
        assert!(
            keeper
                .extend_takeover_on_source_input(lease_id, typing_at)
                .unwrap()
        );
        let renewed = keeper
            .apiary_takeover_audit(10)
            .unwrap()
            .into_iter()
            .find(|entry| entry.lease.id == lease_id)
            .unwrap()
            .lease;
        assert!(
            renewed.expires_at > before,
            "the lease outlives its first five minutes"
        );

        // And a lapsed lease is over: input after expiry revives nothing.
        assert!(
            !keeper
                .extend_takeover_on_source_input(lease_id, renewed.expires_at + 1)
                .unwrap()
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
        let active_lease = active.lease.as_ref().unwrap();
        assert!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &acceptance.node_credential,
                    active_lease.id,
                    active_lease.revision,
                    now + 62,
                )
                .is_ok()
        );
        keeper
            .revoke_stewardship(scope.stewardship.as_ref().unwrap().id, now + 63)
            .expect("revoke");
        assert!(
            keeper
                .authorize_federation_steward_takeover_relay(
                    &acceptance.node_credential,
                    active_lease.id,
                    active_lease.revision,
                    now + 63,
                )
                .is_err()
        );
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
