use std::{collections::HashSet, str::FromStr};

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use swarm_domain::{
    ApiaryHiveCandidate, ApiaryId, ApiaryInvitationBundle, ApiaryInvitationEnvelope,
    ApiaryInvitationEnvelopePayload, ApiaryInvitationId, ApiaryJoinLink, ApiaryJoinLinkBundle,
    ApiaryJoinLinkId, ApiaryJoinLinkPoll, ApiaryJoinLinkState, ApiaryKeeperLink,
    FEDERATION_CATALOG_SCHEMA_VERSION, FEDERATION_CONNECTION_CARD_SCHEMA_VERSION,
    FEDERATION_INVITATION_SCHEMA_VERSION, FEDERATION_MEMBERSHIP_SCHEMA_VERSION,
    FEDERATION_PROTOCOL_VERSION, FederationCatalogAcknowledgement, FederationCatalogSnapshot,
    FederationCatalogSnapshotPayload, FederationClaimId, FederationClaimState,
    FederationJoinAcceptance, FederationJoinInvitation, FederationJoinInvitationState,
    FederationJoinReadiness, FederationJoinSubmission, FederationJoinSubmissionPayload,
    FederationMembershipReceipt, FederationMembershipReceiptId, FederationMembershipReceiptPayload,
    FederationNodeId, FederationProjectManifestEntry, FederationProjectReadiness,
    FederationSharedClaim, FederationSyncCondition, FederationSyncHealth, HiveConnectionCard,
    HiveConnectionCardPayload, HiveId, JiraProjectBindingId, OperatorId, SharedWorkBackend,
    federation_retry_delay_seconds,
};
use url::Url;

use crate::{TaskStore, TaskStoreError, parse_domain_id};

pub const MIN_CONNECTION_CARD_LIFETIME_SECONDS: i64 = 5 * 60;
pub const MAX_CONNECTION_CARD_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const MIN_FEDERATION_INVITATION_LIFETIME_SECONDS: i64 = 5 * 60;
pub const MAX_FEDERATION_INVITATION_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const MAX_ACTIVE_APIARY_JOIN_LINKS: usize = 16;
const MAX_CLOCK_SKEW_SECONDS: i64 = 5 * 60;
const MAX_KEEPER_ENDPOINT_BYTES: usize = 2_048;
const MAX_PROMOTED_PROJECTS_PER_INVITATION: usize = 1_000;
const FEDERATION_NODE_CREDENTIAL_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
const FEDERATION_CATALOG_LIFETIME_SECONDS: i64 = 5 * 60;
const FEDERATION_CLAIM_RESERVATION_SECONDS: i64 = 2 * 60;
const MAX_ACTIVE_FEDERATION_CLAIMS: usize = 1_000;
const FEDERATION_SYNC_INTERVAL_SECONDS: i64 = 60;
const MAX_FEDERATION_SYNC_FAILURES: u32 = 1_000;

struct LocalFederationIdentity {
    node_id: FederationNodeId,
    signing_key: SigningKey,
}

struct KeeperInvitationContext {
    apiary_name: String,
    backend: SharedWorkBackend,
    policy_revision: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredApiaryInvitationBundle {
    keeper_connection_card: HiveConnectionCard,
    invitation: ApiaryInvitationEnvelope,
    promoted_projects: Vec<FederationProjectManifestEntry>,
}

struct KeeperJoinContext {
    apiary_id: String,
    candidate_hive_id: String,
    candidate_node_id: String,
    candidate_operator_id: String,
    secret_digest: Vec<u8>,
    state: String,
    expires_at: i64,
    hive_name: String,
    operator_display_name: String,
    public_key: String,
    envelope_json: String,
}

struct InvitedJoinApplicationContext {
    apiary_id: String,
    apiary_name: String,
    backend: SharedWorkBackend,
    policy_revision: u64,
    catalog_digest: String,
    keeper_node_id: String,
    keeper_hive_id: String,
    keeper_hive_name: String,
    keeper_operator_id: String,
    keeper_operator_display_name: String,
    keeper_public_key: String,
    invited_node_id: String,
    invited_hive_id: String,
    invited_operator_id: String,
    state: String,
}

pub(crate) struct MemberCredentialContext {
    pub(crate) apiary: ApiaryId,
    pub(crate) node: FederationNodeId,
    pub(crate) hive: HiveId,
    pub(crate) operator: OperatorId,
}

impl TaskStore {
    /// Returns the joined Member's host-private outbound Keeper connection.
    /// This is adapter material and must never enter browser or agent reads.
    ///
    /// # Errors
    /// Rejects personal and Keeper Hives, missing or corrupt membership
    /// material, and invalid stored Keeper endpoints.
    pub fn federation_member_connection(
        &self,
    ) -> Result<swarm_domain::FederationMemberConnection, TaskStoreError> {
        self.require_local_federation_member()?;
        let connection = self.connection()?;
        let (keeper_endpoint, credential, credential_expires_at) = connection
            .query_row(
                "SELECT i.keeper_endpoint, m.node_credential, m.credential_expires_at
                 FROM local_federation_membership m
                 JOIN apiary_join_invitations i ON i.id = m.invitation_id
                 WHERE m.singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationSync)?;
        validate_invitation_endpoint(&keeper_endpoint)
            .map_err(|_| TaskStoreError::InvalidFederationSync)?;
        if credential.len() != 32 || credential_expires_at < 0 {
            return Err(TaskStoreError::InvalidFederationSync);
        }
        Ok(swarm_domain::FederationMemberConnection {
            keeper_endpoint,
            node_credential: Base64UrlUnpadded::encode_string(&credential),
            credential_expires_at,
        })
    }

    /// Returns content-free local Member synchronization health. The absence of
    /// a row means that a transport loop has never run on this installation.
    ///
    /// # Errors
    /// Rejects personal and Keeper Hives, corrupt state, and unavailable persistence.
    pub fn federation_sync_health(&self) -> Result<FederationSyncHealth, TaskStoreError> {
        self.require_local_federation_member()?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT condition, last_attempt_at, last_success_at,
                        consecutive_failures, next_attempt_at
                 FROM local_federation_sync WHERE singleton = 1",
                [],
                federation_sync_health_from_row,
            )
            .optional()?
            .map_or(Ok(FederationSyncHealth::default()), Ok)
    }

    /// Records one successful bounded Member reconciliation without retaining
    /// endpoints, credentials, response bodies, or shared-work content.
    ///
    /// # Errors
    /// Rejects personal and Keeper Hives, invalid time, and persistence failures.
    pub fn record_federation_sync_success(
        &self,
        now: i64,
    ) -> Result<FederationSyncHealth, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationSync);
        }
        self.require_local_federation_member()?;
        let health = FederationSyncHealth {
            condition: FederationSyncCondition::Current,
            last_attempt_at: Some(now),
            last_success_at: Some(now),
            consecutive_failures: 0,
            next_attempt_at: Some(now.saturating_add(FEDERATION_SYNC_INTERVAL_SECONDS)),
        };
        self.persist_federation_sync_health(&health, now)?;
        Ok(health)
    }

    /// Records a classified Member reconciliation failure. Temporary outages
    /// back off to five minutes; authentication and protocol failures halt.
    ///
    /// # Errors
    /// Rejects success/idle conditions, personal and Keeper Hives, invalid
    /// time, corrupt state, and persistence failures.
    pub fn record_federation_sync_failure(
        &self,
        condition: FederationSyncCondition,
        now: i64,
    ) -> Result<FederationSyncHealth, TaskStoreError> {
        if now < 0
            || !matches!(
                condition,
                FederationSyncCondition::Offline
                    | FederationSyncCondition::AuthenticationRequired
                    | FederationSyncCondition::Incompatible
            )
        {
            return Err(TaskStoreError::InvalidFederationSync);
        }
        self.require_local_federation_member()?;
        let prior = self.federation_sync_health()?;
        let consecutive_failures = prior
            .consecutive_failures
            .saturating_add(1)
            .min(MAX_FEDERATION_SYNC_FAILURES);
        let next_attempt_at = (condition == FederationSyncCondition::Offline)
            .then(|| now.saturating_add(federation_retry_delay_seconds(consecutive_failures)));
        let health = FederationSyncHealth {
            condition,
            last_attempt_at: Some(now),
            last_success_at: prior.last_success_at,
            consecutive_failures,
            next_attempt_at,
        };
        self.persist_federation_sync_health(&health, now)?;
        Ok(health)
    }

    fn require_local_federation_member(&self) -> Result<(), TaskStoreError> {
        match self.local_apiary_context()? {
            swarm_domain::LocalApiaryContext::Federated {
                local_role: swarm_domain::LocalApiaryRole::Member,
                ..
            } => Ok(()),
            _ => Err(TaskStoreError::InvalidFederationSync),
        }
    }

    fn persist_federation_sync_health(
        &self,
        health: &FederationSyncHealth,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "INSERT INTO local_federation_sync
                (singleton, condition, last_attempt_at, last_success_at,
                 consecutive_failures, next_attempt_at, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(singleton) DO UPDATE SET
                 condition = excluded.condition,
                 last_attempt_at = excluded.last_attempt_at,
                 last_success_at = excluded.last_success_at,
                 consecutive_failures = excluded.consecutive_failures,
                 next_attempt_at = excluded.next_attempt_at,
                 updated_at = excluded.updated_at",
            params![
                health.condition.to_string(),
                health.last_attempt_at,
                health.last_success_at,
                health.consecutive_failures,
                health.next_attempt_at,
                now,
            ],
        )?;
        Ok(())
    }

    /// Lists the current Apiary's active shared-work reservations and durable
    /// home-Hive claims for the Keeper control room. Expired reservations and
    /// released history are deliberately omitted so routine reconciliation
    /// noise does not enter the operator rollup.
    ///
    /// # Errors
    /// Rejects personal and Member Hives, invalid time, corrupt records, and
    /// unavailable persistence.
    pub fn list_active_federation_claims(
        &self,
        now: i64,
    ) -> Result<Vec<FederationSharedClaim>, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationClaim);
        }
        let identity = self.local_hive_identity()?;
        let context = self.local_apiary_context()?;
        let swarm_domain::LocalApiaryContext::Federated { apiary, local_role } = context else {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        };
        if local_role != swarm_domain::LocalApiaryRole::Keeper
            || apiary.keeper_operator_id != identity.operator.id
        {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, apiary_id, project_id, issue_id, issue_key,
                    home_node_id, home_hive_id, home_operator_id, state,
                    reserved_at, reservation_expires_at, confirmed_at, released_at
             FROM apiary_federation_claims
             WHERE apiary_id = ?1
               AND (state = 'confirmed'
                    OR (state = 'reserved' AND reservation_expires_at > ?2))
             ORDER BY CASE state WHEN 'reserved' THEN 0 ELSE 1 END,
                      COALESCE(confirmed_at, reserved_at) DESC, issue_key ASC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![apiary.id.to_string(), now, MAX_ACTIVE_FEDERATION_CLAIMS],
            federation_claim_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Atomically reserves one promoted Jira issue for the authenticated member
    /// Hive. Exact retries by that Hive return the same claim; another Hive
    /// fails closed until an unconfirmed reservation expires or is released.
    ///
    /// # Errors
    /// Rejects invalid credentials or issue identity, non-promoted projects,
    /// conflicting ownership, and unavailable persistence.
    pub fn reserve_federation_claim(
        &self,
        node_credential: &str,
        project_id: &str,
        issue_id: &str,
        issue_key: &str,
        now: i64,
    ) -> Result<FederationSharedClaim, TaskStoreError> {
        validate_claim_identity(project_id, issue_id, issue_key, now)?;
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        let project_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM apiary_jira_projects
             WHERE apiary_id = ?1 AND project_id = ?2)",
            params![member.apiary.to_string(), project_id.trim()],
            |row| row.get::<_, bool>(0),
        )?;
        if !project_exists {
            return Err(TaskStoreError::InvalidFederationClaim);
        }
        transaction.execute(
            "UPDATE apiary_federation_claims
             SET state = 'expired', updated_at = ?1
             WHERE apiary_id = ?2 AND project_id = ?3 AND issue_id = ?4
               AND state = 'reserved' AND reservation_expires_at <= ?1",
            params![
                now,
                member.apiary.to_string(),
                project_id.trim(),
                issue_id.trim()
            ],
        )?;
        if let Some(existing) = active_claim_for_issue(
            &transaction,
            member.apiary,
            project_id.trim(),
            issue_id.trim(),
        )? {
            if existing.home_node_id == member.node {
                return Ok(existing);
            }
            return Err(TaskStoreError::FederationClaimConflict);
        }
        let claim = FederationSharedClaim {
            id: FederationClaimId::new(),
            apiary_id: member.apiary,
            project_id: project_id.trim().to_owned(),
            issue_id: issue_id.trim().to_owned(),
            issue_key: issue_key.trim().to_owned(),
            home_node_id: member.node,
            home_hive_id: member.hive,
            home_operator_id: member.operator,
            state: FederationClaimState::Reserved,
            reserved_at: now,
            reservation_expires_at: now
                .checked_add(FEDERATION_CLAIM_RESERVATION_SECONDS)
                .ok_or(TaskStoreError::InvalidFederationClaim)?,
            confirmed_at: None,
            released_at: None,
        };
        insert_federation_claim(&transaction, &claim, now)?;
        transaction.commit()?;
        Ok(claim)
    }

    /// Confirms that the authenticated member completed its Jira assignment.
    /// Confirmation makes home-Hive ownership durable beyond the reservation.
    ///
    /// # Errors
    /// Rejects wrong members, expired/released claims, invalid credentials,
    /// and unavailable persistence.
    pub fn confirm_federation_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        now: i64,
    ) -> Result<FederationSharedClaim, TaskStoreError> {
        self.resolve_federation_claim(node_credential, claim_id, now, true)
    }

    /// Releases an unconfirmed reservation owned by the authenticated member.
    /// Confirmed work requires the later explicit handoff/release workflow.
    ///
    /// # Errors
    /// Rejects wrong members, confirmed/expired claims, invalid credentials,
    /// and unavailable persistence.
    pub fn release_federation_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        now: i64,
    ) -> Result<FederationSharedClaim, TaskStoreError> {
        self.resolve_federation_claim(node_credential, claim_id, now, false)
    }

    fn resolve_federation_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        now: i64,
        confirm: bool,
    ) -> Result<FederationSharedClaim, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationClaim);
        }
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        let mut claim = federation_claim_by_id(&transaction, claim_id)?
            .ok_or(TaskStoreError::InvalidFederationClaim)?;
        if claim.apiary_id != member.apiary || claim.home_node_id != member.node {
            return Err(TaskStoreError::FederationClaimConflict);
        }
        let target = if confirm {
            FederationClaimState::Confirmed
        } else {
            FederationClaimState::Released
        };
        if claim.state == target {
            return Ok(claim);
        }
        if claim.state != FederationClaimState::Reserved || claim.reservation_expires_at <= now {
            if claim.state == FederationClaimState::Reserved {
                transaction.execute(
                    "UPDATE apiary_federation_claims SET state = 'expired', updated_at = ?1
                     WHERE id = ?2 AND state = 'reserved'",
                    params![now, claim_id.to_string()],
                )?;
                transaction.commit()?;
            }
            return Err(TaskStoreError::InvalidFederationClaim);
        }
        let changed = if confirm {
            transaction.execute(
                "UPDATE apiary_federation_claims
                 SET state = 'confirmed', confirmed_at = ?1, updated_at = ?1
                 WHERE id = ?2 AND state = 'reserved' AND reservation_expires_at > ?1",
                params![now, claim_id.to_string()],
            )?
        } else {
            transaction.execute(
                "UPDATE apiary_federation_claims
                 SET state = 'released', released_at = ?1, updated_at = ?1
                 WHERE id = ?2 AND state = 'reserved' AND reservation_expires_at > ?1",
                params![now, claim_id.to_string()],
            )?
        };
        if changed != 1 {
            return Err(TaskStoreError::FederationClaimConflict);
        }
        claim = federation_claim_by_id(&transaction, claim_id)?
            .ok_or(TaskStoreError::InvalidFederationClaim)?;
        transaction.commit()?;
        Ok(claim)
    }

    /// Verifies and atomically acknowledges one Keeper-signed catalog on the
    /// exact joined Member Hive. Exact retries are idempotent; older snapshots
    /// cannot replace newer evidence.
    ///
    /// # Errors
    /// Rejects Keepers and personal Hives, invalid or expired membership,
    /// wrong-federation signatures, stale snapshots, and persistence failures.
    pub fn acknowledge_federation_catalog(
        &self,
        snapshot: &FederationCatalogSnapshot,
        now: i64,
    ) -> Result<FederationCatalogAcknowledgement, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationCatalog);
        }
        let identity = self.local_hive_identity()?;
        let context = self.local_apiary_context()?;
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            return Err(TaskStoreError::InvalidFederationCatalog);
        };
        if apiary.keeper_operator_id == identity.operator.id {
            return Err(TaskStoreError::InvalidFederationCatalog);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (receipt_json, keeper_public_key, credential_expires_at) = transaction
            .query_row(
                "SELECT m.receipt_json, i.keeper_public_key, m.credential_expires_at
                 FROM local_federation_membership m
                 JOIN apiary_join_invitations i ON i.id = m.invitation_id
                 WHERE m.singleton = 1 AND m.apiary_id = ?1",
                [apiary.id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationCatalog)?;
        if credential_expires_at <= now {
            return Err(TaskStoreError::InvalidFederationCatalog);
        }
        let receipt: FederationMembershipReceipt = serde_json::from_str(&receipt_json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        verify_federation_membership_receipt(&receipt, &keeper_public_key, now)?;
        verify_federation_catalog_snapshot(snapshot, &keeper_public_key, &receipt, now)?;
        let serialized = serde_json::to_string(snapshot)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let existing = transaction
            .query_row(
                "SELECT snapshot_json, snapshot_issued_at, acknowledged_at
                 FROM local_federation_catalog WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((existing_json, issued_at, acknowledged_at)) = existing {
            if existing_json == serialized {
                return Ok(catalog_acknowledgement(snapshot, acknowledged_at));
            }
            if snapshot.payload.issued_at <= issued_at {
                return Err(TaskStoreError::InvalidFederationCatalog);
            }
        }
        transaction.execute(
            "INSERT INTO local_federation_catalog
                (singleton, apiary_id, policy_revision, catalog_digest,
                 project_count, snapshot_json, snapshot_issued_at,
                 snapshot_expires_at, acknowledged_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(singleton) DO UPDATE SET
                 apiary_id = excluded.apiary_id,
                 policy_revision = excluded.policy_revision,
                 catalog_digest = excluded.catalog_digest,
                 project_count = excluded.project_count,
                 snapshot_json = excluded.snapshot_json,
                 snapshot_issued_at = excluded.snapshot_issued_at,
                 snapshot_expires_at = excluded.snapshot_expires_at,
                 acknowledged_at = excluded.acknowledged_at",
            params![
                snapshot.payload.apiary_id.to_string(),
                snapshot.payload.policy_revision,
                &snapshot.payload.promoted_project_catalog_digest,
                snapshot.payload.projects.len(),
                serialized,
                snapshot.payload.issued_at,
                snapshot.payload.expires_at,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(catalog_acknowledgement(snapshot, now))
    }

    /// Returns the latest locally verified Keeper catalog acknowledgement.
    ///
    /// # Errors
    /// Returns an error when durable state is unavailable or corrupt.
    pub fn federation_catalog_acknowledgement(
        &self,
    ) -> Result<Option<FederationCatalogAcknowledgement>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT apiary_id, policy_revision, catalog_digest,
                        project_count, snapshot_issued_at, snapshot_expires_at,
                        acknowledged_at
                 FROM local_federation_catalog WHERE singleton = 1",
                [],
                |row| {
                    Ok(FederationCatalogAcknowledgement {
                        apiary_id: parse_domain_id(&row.get::<_, String>(0)?)?,
                        policy_revision: row.get(1)?,
                        promoted_project_catalog_digest: row.get(2)?,
                        project_count: row.get(3)?,
                        snapshot_issued_at: row.get(4)?,
                        snapshot_expires_at: row.get(5)?,
                        acknowledged_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Compares every project in the latest verified Keeper snapshot with
    /// this Hive's private Jira binding and workflow evidence.
    ///
    /// # Errors
    /// Returns an error for corrupt snapshot state or unavailable persistence.
    pub fn acknowledged_federation_project_readiness(
        &self,
    ) -> Result<Vec<FederationProjectReadiness>, TaskStoreError> {
        let connection = self.connection()?;
        let snapshot_json = connection
            .query_row(
                "SELECT snapshot_json FROM local_federation_catalog WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        drop(connection);
        let Some(snapshot_json) = snapshot_json else {
            return Ok(Vec::new());
        };
        let snapshot: FederationCatalogSnapshot = serde_json::from_str(&snapshot_json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let bindings = self.list_jira_project_bindings()?;
        Ok(snapshot
            .payload
            .projects
            .into_iter()
            .map(|project| {
                let binding = bindings
                    .iter()
                    .find(|binding| binding.project_id == project.project_id);
                FederationProjectReadiness {
                    project,
                    binding_id: binding.map(|binding| binding.id),
                    access_verified: binding.is_some_and(|binding| binding.access_verified),
                    workflow_mapped: binding.is_some_and(|binding| binding.workflow_mapped),
                }
            })
            .collect())
    }

    /// Returns a short-lived Keeper-signed public project catalog bound to the
    /// exact joined member node authenticated by `node_credential`.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown or expired credentials, identity drift,
    /// invalid time, and unavailable persistence.
    pub fn signed_federation_catalog(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationCatalogSnapshot, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let credential: [u8; 32] = Base64UrlUnpadded::decode_vec(node_credential)
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let local_node = self.local_federation_identity(now)?;
        let connection = self.connection()?;
        let context =
            keeper_invitation_context(&connection, apiary_id, &identity.operator.id.to_string())?;
        let credential_digest = invitation_secret_digest(&credential);
        let member_node_id = connection
            .query_row(
                "SELECT member_node_id FROM apiary_federation_memberships
                 WHERE apiary_id = ?1 AND credential_digest = ?2
                   AND credential_expires_at > ?3",
                params![apiary_id.to_string(), credential_digest.as_slice(), now],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationCredential)?;
        let projects = promoted_project_manifest(&connection, apiary_id)?;
        let payload = FederationCatalogSnapshotPayload {
            schema_version: FEDERATION_CATALOG_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            apiary_id,
            policy_revision: context.policy_revision,
            promoted_project_catalog_digest: promoted_project_manifest_digest(&projects)?,
            projects,
            keeper_node_id: local_node.node_id,
            keeper_hive_id: identity.hive.id,
            keeper_operator_id: identity.operator.id,
            member_node_id: parse_domain_id(&member_node_id)?,
            issued_at: now,
            expires_at: now
                .checked_add(FEDERATION_CATALOG_LIFETIME_SECONDS)
                .ok_or(TaskStoreError::InvalidFederationCredential)?,
        };
        let signature = local_node
            .signing_key
            .sign(&canonical_catalog_snapshot_payload(&payload)?);
        Ok(FederationCatalogSnapshot {
            payload,
            signature: Base64UrlUnpadded::encode_string(&signature.to_bytes()),
        })
    }

    /// Issues a short-lived, self-authenticating public identity document for
    /// the local Hive. It contains no endpoint, credential, repository, task,
    /// terminal, or integration material.
    ///
    /// # Errors
    /// Returns an error for invalid time bounds, unavailable entropy, corrupt
    /// durable identity material, or persistence failures.
    pub fn issue_hive_connection_card(
        &self,
        now: i64,
        lifetime_seconds: i64,
    ) -> Result<HiveConnectionCard, TaskStoreError> {
        if now < 0
            || !(MIN_CONNECTION_CARD_LIFETIME_SECONDS..=MAX_CONNECTION_CARD_LIFETIME_SECONDS)
                .contains(&lifetime_seconds)
        {
            return Err(TaskStoreError::InvalidFederationConnectionCard);
        }
        let identity = self.local_hive_identity()?;
        let local_node = self.local_federation_identity(now)?;
        connection_card_for(&identity, &local_node, now, lifetime_seconds)
    }

    /// Creates one bounded Keeper URL capability. Only its domain-separated
    /// digest is durable; the plaintext secret is returned once for placement
    /// in the URL fragment.
    ///
    /// # Errors
    /// Rejects non-Keepers, invalid endpoints or bounds, capacity exhaustion,
    /// entropy failure, and unavailable persistence.
    pub fn issue_apiary_join_link(
        &self,
        keeper_endpoint: &str,
        now: i64,
        lifetime_seconds: i64,
    ) -> Result<ApiaryJoinLinkBundle, TaskStoreError> {
        validate_invitation_endpoint(keeper_endpoint)?;
        if now < 0
            || !(MIN_FEDERATION_INVITATION_LIFETIME_SECONDS
                ..=MAX_FEDERATION_INVITATION_LIFETIME_SECONDS)
                .contains(&lifetime_seconds)
        {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let (secret, one_time_secret) = bearer_secret_material()?;
        let secret_digest = join_link_secret_digest(&secret);
        let id = ApiaryJoinLinkId::new();
        let expires_at = now
            .checked_add(lifetime_seconds)
            .ok_or(TaskStoreError::InvalidApiaryJoinLink)?;
        let endpoint = keeper_endpoint.trim().trim_end_matches('/').to_owned();

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let context =
            keeper_invitation_context(&transaction, apiary_id, &identity.operator.id.to_string())?;
        let active = transaction.query_row(
            "SELECT COUNT(*) FROM apiary_join_links
             WHERE apiary_id = ?1
               AND state IN ('open','awaiting_approval','approved')
               AND expires_at > ?2",
            params![apiary_id.to_string(), now],
            |row| row.get::<_, usize>(0),
        )?;
        if active >= MAX_ACTIVE_APIARY_JOIN_LINKS {
            return Err(TaskStoreError::ApiaryJoinLinkLimit);
        }
        transaction.execute(
            "INSERT INTO apiary_join_links
                (id, apiary_id, created_by_operator_id, keeper_endpoint,
                 secret_digest, state, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7)",
            params![
                id.to_string(),
                apiary_id.to_string(),
                identity.operator.id.to_string(),
                &endpoint,
                secret_digest.as_slice(),
                now,
                expires_at,
            ],
        )?;
        transaction.commit()?;
        Ok(ApiaryJoinLinkBundle {
            link: ApiaryJoinLink {
                id,
                apiary_id,
                apiary_name: context.apiary_name,
                keeper_endpoint: endpoint,
                state: ApiaryJoinLinkState::Open,
                candidate: None,
                issued_at: now,
                expires_at,
            },
            one_time_secret,
        })
    }

    /// Lists public Keeper-side join-link state without bearer material.
    ///
    /// # Errors
    /// Rejects non-Keepers and unavailable or corrupt persistence.
    pub fn apiary_join_links(&self, now: i64) -> Result<Vec<ApiaryJoinLink>, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let connection = self.connection()?;
        keeper_invitation_context(&connection, apiary_id, &identity.operator.id.to_string())?;
        load_apiary_join_links(&connection, apiary_id, now)
    }

    /// Saves one Keeper-created capability inside the personal Hive so the API
    /// can present and poll it server-to-server across browser reloads. The
    /// plaintext secret remains private and never appears in list results.
    ///
    /// # Errors
    /// Rejects non-personal Hives, malformed capabilities, duplicates, and
    /// unavailable persistence.
    pub fn save_local_apiary_keeper_link(
        &self,
        link_id: ApiaryJoinLinkId,
        keeper_endpoint: &str,
        one_time_secret: &str,
        now: i64,
    ) -> Result<ApiaryKeeperLink, TaskStoreError> {
        validate_invitation_endpoint(keeper_endpoint)
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?;
        if now < 0 || self.local_hive_identity()?.hive.apiary_id.is_some() {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        let secret: [u8; 32] = Base64UrlUnpadded::decode_vec(one_time_secret)
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?;
        let endpoint = keeper_endpoint.trim().trim_end_matches('/').to_owned();
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO local_apiary_keeper_links
                    (link_id, keeper_endpoint, one_time_secret, state,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'open', ?4, ?4)",
                params![link_id.to_string(), endpoint, secret.as_slice(), now],
            )
            .map_err(|error| {
                if matches!(error, rusqlite::Error::SqliteFailure(_, _)) {
                    TaskStoreError::ApiaryJoinLinkResolved
                } else {
                    TaskStoreError::Sql(error)
                }
            })?;
        load_local_apiary_keeper_link(&connection, link_id)?
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)
    }

    /// Lists pending outbound Keeper connections without bearer secrets.
    ///
    /// # Errors
    /// Returns an error when private local persistence is unavailable.
    pub fn local_apiary_keeper_links(&self) -> Result<Vec<ApiaryKeeperLink>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT link_id, keeper_endpoint, apiary_name, state,
                    created_at, updated_at, expires_at
             FROM local_apiary_keeper_links
             ORDER BY created_at DESC, link_id",
        )?;
        statement
            .query_map([], local_apiary_keeper_link_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns one private outbound credential for server-side polling only.
    /// Callers must never serialize the returned secret to a browser response.
    ///
    /// # Errors
    /// Rejects unknown links and unavailable or corrupt persistence.
    pub fn local_apiary_keeper_link_credential(
        &self,
        link_id: ApiaryJoinLinkId,
    ) -> Result<(String, String), TaskStoreError> {
        let connection = self.connection()?;
        let (endpoint, secret): (String, Vec<u8>) = connection
            .query_row(
                "SELECT keeper_endpoint, one_time_secret
                 FROM local_apiary_keeper_links WHERE link_id = ?1",
                [link_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)?;
        let secret: [u8; 32] = secret
            .try_into()
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?;
        Ok((endpoint, Base64UrlUnpadded::encode_string(&secret)))
    }

    /// Records only the signed Keeper response metadata after one outbound
    /// poll, preserving the original endpoint and secret binding.
    ///
    /// # Errors
    /// Rejects endpoint or identity substitution and unavailable persistence.
    pub fn update_local_apiary_keeper_link(
        &self,
        remote: &ApiaryJoinLink,
        now: i64,
    ) -> Result<ApiaryKeeperLink, TaskStoreError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE local_apiary_keeper_links
             SET apiary_name = ?2, state = ?3, updated_at = ?4, expires_at = ?5
             WHERE link_id = ?1 AND keeper_endpoint = ?6",
            params![
                remote.id.to_string(),
                &remote.apiary_name,
                remote.state.to_string(),
                now,
                remote.expires_at,
                &remote.keeper_endpoint,
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        load_local_apiary_keeper_link(&connection, remote.id)?
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)
    }

    /// Removes a completed local bootstrap after its signed invitation has
    /// been imported into the existing durable invitation workflow.
    ///
    /// # Errors
    /// Rejects unknown links and unavailable persistence.
    pub fn remove_local_apiary_keeper_link(
        &self,
        link_id: ApiaryJoinLinkId,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        if connection.execute(
            "DELETE FROM local_apiary_keeper_links WHERE link_id = ?1",
            [link_id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryJoinLinkNotFound);
        }
        Ok(())
    }

    /// Authenticates a public bootstrap request and binds it permanently to the
    /// exact signed Hive identity that redeemed the URL. Replays by that same
    /// identity are idempotent; substitution fails closed.
    ///
    /// # Errors
    /// Rejects invalid/expired capabilities, identity substitution, non-Keeper
    /// state, invalid cards, and unavailable persistence.
    pub fn present_apiary_join_link_identity(
        &self,
        link_id: ApiaryJoinLinkId,
        one_time_secret: &str,
        card: &HiveConnectionCard,
        now: i64,
    ) -> Result<ApiaryJoinLink, TaskStoreError> {
        let link = self.authenticate_apiary_join_link(link_id, one_time_secret, now)?;
        if !matches!(
            link.state,
            ApiaryJoinLinkState::Open | ApiaryJoinLinkState::AwaitingApproval
        ) {
            return Err(TaskStoreError::ApiaryJoinLinkResolved);
        }
        if link
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.hive_id != card.payload.hive_id)
        {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        if link.candidate.is_some() {
            return Ok(link);
        }
        let candidate = self.pin_hive_candidate(card, now)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE apiary_join_links
             SET candidate_hive_id = ?2, state = 'awaiting_approval'
             WHERE id = ?1 AND apiary_id = ?3 AND expires_at > ?4
               AND state IN ('open','awaiting_approval')
               AND (candidate_hive_id IS NULL OR candidate_hive_id = ?2)",
            params![
                link_id.to_string(),
                candidate.hive_id.to_string(),
                link.apiary_id.to_string(),
                now,
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::ApiaryJoinLinkResolved);
        }
        drop(connection);
        self.authenticate_apiary_join_link(link_id, one_time_secret, now)
    }

    /// Explicitly approves the exact bound Hive identity. Approval does not
    /// create membership or issue an invitation until that Hive polls again.
    ///
    /// # Errors
    /// Rejects non-Keepers, unbound/resolved/expired links, and unavailable
    /// persistence.
    pub fn approve_apiary_join_link(
        &self,
        link_id: ApiaryJoinLinkId,
        now: i64,
    ) -> Result<ApiaryJoinLink, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let connection = self.connection()?;
        keeper_invitation_context(&connection, apiary_id, &identity.operator.id.to_string())?;
        let changed = connection.execute(
            "UPDATE apiary_join_links
             SET state = 'approved', approved_at = ?3
             WHERE id = ?1 AND apiary_id = ?2 AND state = 'awaiting_approval'
               AND candidate_hive_id IS NOT NULL AND expires_at > ?3",
            params![link_id.to_string(), apiary_id.to_string(), now],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::ApiaryJoinLinkResolved);
        }
        load_apiary_join_links(&connection, apiary_id, now)?
            .into_iter()
            .find(|link| link.id == link_id)
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)
    }

    /// Returns the current bootstrap state to the member Hive. Once the Keeper
    /// approves the bound identity, the first poll issues an invitation using
    /// the same bearer secret and later polls reconstruct that exact public
    /// bundle without storing or rotating the secret.
    ///
    /// # Errors
    /// Rejects an invalid/expired capability, missing candidate identity,
    /// corrupt retry material, and invitation issuance failures.
    pub fn poll_apiary_join_link(
        &self,
        link_id: ApiaryJoinLinkId,
        one_time_secret: &str,
        now: i64,
    ) -> Result<ApiaryJoinLinkPoll, TaskStoreError> {
        let link = self.authenticate_apiary_join_link(link_id, one_time_secret, now)?;
        if link.state == ApiaryJoinLinkState::InvitationIssued {
            let connection = self.connection()?;
            let stored = connection
                .query_row(
                    "SELECT invitation_bundle_json FROM apiary_join_links WHERE id = ?1",
                    [link_id.to_string()],
                    |row| row.get::<_, Option<String>>(0),
                )?
                .ok_or(TaskStoreError::InvalidApiaryJoinLink)?;
            let stored: StoredApiaryInvitationBundle = serde_json::from_str(&stored)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            return Ok(ApiaryJoinLinkPoll {
                link,
                invitation: Some(ApiaryInvitationBundle {
                    keeper_connection_card: stored.keeper_connection_card,
                    invitation: stored.invitation,
                    promoted_projects: stored.promoted_projects,
                    one_time_secret: one_time_secret.to_owned(),
                }),
            });
        }
        if link.state != ApiaryJoinLinkState::Approved {
            return Ok(ApiaryJoinLinkPoll {
                link,
                invitation: None,
            });
        }

        let candidate = link
            .candidate
            .as_ref()
            .ok_or(TaskStoreError::InvalidApiaryJoinLink)?;
        let secret: [u8; 32] = Base64UrlUnpadded::decode_vec(one_time_secret)
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?;
        let lifetime_seconds = link.expires_at.saturating_sub(now);
        let mut nonce = [0_u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
        let invitation = self.issue_apiary_invitation_bundle_with_secret(
            candidate.hive_id,
            &link.keeper_endpoint,
            now,
            lifetime_seconds,
            secret,
            one_time_secret.to_owned(),
            Base64UrlUnpadded::encode_string(&nonce),
        )?;
        let stored = StoredApiaryInvitationBundle {
            keeper_connection_card: invitation.keeper_connection_card.clone(),
            invitation: invitation.invitation.clone(),
            promoted_projects: invitation.promoted_projects.clone(),
        };
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE apiary_join_links
             SET state = 'invitation_issued', invitation_id = ?2,
                 invitation_bundle_json = ?3
             WHERE id = ?1 AND state = 'approved' AND expires_at > ?4",
            params![
                link_id.to_string(),
                invitation.invitation.payload.invitation_id.to_string(),
                serde_json::to_string(&stored)
                    .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
                now,
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::ApiaryJoinLinkResolved);
        }
        drop(connection);
        let completed = self.authenticate_apiary_join_link(link_id, one_time_secret, now)?;
        Ok(ApiaryJoinLinkPoll {
            link: completed,
            invitation: Some(invitation),
        })
    }

    fn authenticate_apiary_join_link(
        &self,
        link_id: ApiaryJoinLinkId,
        one_time_secret: &str,
        now: i64,
    ) -> Result<ApiaryJoinLink, TaskStoreError> {
        let secret: [u8; 32] = Base64UrlUnpadded::decode_vec(one_time_secret)
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?;
        let presented = join_link_secret_digest(&secret);
        let connection = self.connection()?;
        let (apiary_id, expected): (String, Vec<u8>) = connection
            .query_row(
                "SELECT apiary_id, secret_digest FROM apiary_join_links WHERE id = ?1",
                [link_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)?;
        if expected.len() != presented.len()
            || !bool::from(expected.as_slice().ct_eq(presented.as_slice()))
        {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        let apiary_id = parse_domain_id(&apiary_id)?;
        let link = load_apiary_join_links(&connection, apiary_id, now)?
            .into_iter()
            .find(|link| link.id == link_id)
            .ok_or(TaskStoreError::ApiaryJoinLinkNotFound)?;
        if link.state == ApiaryJoinLinkState::Expired {
            return Err(TaskStoreError::InvalidApiaryJoinLink);
        }
        Ok(link)
    }

    /// Creates a signed, bounded invitation for one already pinned Hive. The
    /// returned bearer secret exists only in this response; Keeper storage
    /// receives its SHA-256 digest in the same transaction as the envelope.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown candidates, invalid endpoints or bounds,
    /// duplicate pending invitations, corrupt identity state, and persistence
    /// failures.
    pub fn issue_apiary_invitation_bundle(
        &self,
        invited_hive_id: HiveId,
        keeper_endpoint: &str,
        now: i64,
        lifetime_seconds: i64,
    ) -> Result<ApiaryInvitationBundle, TaskStoreError> {
        let (secret, one_time_secret, nonce) = invitation_material()?;
        self.issue_apiary_invitation_bundle_with_secret(
            invited_hive_id,
            keeper_endpoint,
            now,
            lifetime_seconds,
            secret,
            one_time_secret,
            nonce,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn issue_apiary_invitation_bundle_with_secret(
        &self,
        invited_hive_id: HiveId,
        keeper_endpoint: &str,
        now: i64,
        lifetime_seconds: i64,
        secret: [u8; 32],
        one_time_secret: String,
        nonce: String,
    ) -> Result<ApiaryInvitationBundle, TaskStoreError> {
        validate_invitation_endpoint(keeper_endpoint)?;
        if now < 0
            || !(MIN_FEDERATION_INVITATION_LIFETIME_SECONDS
                ..=MAX_FEDERATION_INVITATION_LIFETIME_SECONDS)
                .contains(&lifetime_seconds)
        {
            return Err(TaskStoreError::InvalidFederationInvitation);
        }
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let local_node = self.local_federation_identity(now)?;
        let keeper_connection_card =
            connection_card_for(&identity, &local_node, now, lifetime_seconds)?;
        let secret_digest = invitation_secret_digest(&secret);

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let context =
            keeper_invitation_context(&transaction, apiary_id, &identity.operator.id.to_string())?;
        let candidate = candidate_by_hive(&transaction, apiary_id, invited_hive_id)?
            .ok_or(TaskStoreError::HiveCandidateNotFound)?;
        let invitation_id = ApiaryInvitationId::new();
        let expires_at = now
            .checked_add(lifetime_seconds)
            .ok_or(TaskStoreError::InvalidFederationInvitation)?;
        let promoted_projects = promoted_project_manifest(&transaction, apiary_id)?;
        let payload = ApiaryInvitationEnvelopePayload {
            schema_version: FEDERATION_INVITATION_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            invitation_id,
            apiary_id,
            apiary_name: context.apiary_name,
            shared_work_backend: context.backend,
            required_policy_revision: context.policy_revision,
            promoted_project_catalog_digest: promoted_project_manifest_digest(&promoted_projects)?,
            keeper_node_id: local_node.node_id,
            keeper_hive_id: identity.hive.id,
            keeper_operator_id: identity.operator.id,
            invited_node_id: candidate.node_id,
            invited_hive_id: candidate.hive_id,
            invited_operator_id: candidate.operator_id,
            keeper_endpoint: keeper_endpoint.trim().trim_end_matches('/').to_owned(),
            issued_at: now,
            expires_at,
            nonce,
        };
        let signature = local_node
            .signing_key
            .sign(&canonical_invitation_payload(&payload)?);
        let invitation = ApiaryInvitationEnvelope {
            payload,
            signature: Base64UrlUnpadded::encode_string(&signature.to_bytes()),
        };
        transaction
            .execute(
                "INSERT INTO apiary_federation_invitations
                    (id, apiary_id, candidate_hive_id, candidate_node_id,
                     candidate_operator_id, invited_by_operator_id, secret_digest,
                     nonce, envelope_json, state, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10, ?11)",
                params![
                    invitation_id.to_string(),
                    apiary_id.to_string(),
                    candidate.hive_id.to_string(),
                    candidate.node_id.to_string(),
                    candidate.operator_id.to_string(),
                    identity.operator.id.to_string(),
                    secret_digest.as_slice(),
                    &invitation.payload.nonce,
                    serde_json::to_string(&invitation)
                        .map_err(|error| { TaskStoreError::IntegrityFailure(error.to_string()) })?,
                    now,
                    expires_at,
                ],
            )
            .map_err(map_federation_invitation_insert_error)?;
        transaction.commit()?;
        Ok(ApiaryInvitationBundle {
            keeper_connection_card,
            invitation,
            promoted_projects,
            one_time_secret,
        })
    }

    /// Verifies an invitation addressed to this exact personal Hive, pins the
    /// Keeper identity carried by its independently signed connection card, and
    /// durably retains the one-time secret for the later join handshake.
    /// Import grants no membership and accepts no Apiary policy.
    ///
    /// # Errors
    /// Rejects invalid signatures, expired material, identity mismatches,
    /// unsupported backends, non-personal Hives, duplicates, and persistence
    /// failures.
    pub fn import_apiary_invitation_bundle(
        &self,
        bundle: &ApiaryInvitationBundle,
        now: i64,
    ) -> Result<FederationJoinInvitation, TaskStoreError> {
        verify_hive_connection_card(&bundle.keeper_connection_card, now)?;
        verify_apiary_invitation_envelope(
            &bundle.invitation,
            &bundle.keeper_connection_card.payload.public_key,
            now,
        )?;
        let identity = self.local_hive_identity()?;
        if identity.hive.apiary_id.is_some() {
            return Err(TaskStoreError::ApiaryMembershipConflict);
        }
        let local_node = self.local_federation_identity(now)?;
        let secret = validate_imported_invitation_bindings(bundle, &identity, local_node.node_id)?;
        let invitation = federation_join_invitation(bundle, now);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_personal = transaction.query_row(
            "SELECT apiary_id IS NULL FROM hives WHERE id = ?1",
            [identity.hive.id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        if !still_personal {
            return Err(TaskStoreError::ApiaryMembershipConflict);
        }
        transaction
            .execute(
                "INSERT INTO apiary_join_invitations
                    (id, apiary_id, apiary_name, shared_work_backend,
                     required_policy_revision, promoted_project_catalog_digest,
                     keeper_node_id, keeper_hive_id, keeper_hive_name,
                     keeper_operator_id, keeper_operator_display_name,
                     keeper_public_key, keeper_endpoint, invited_node_id,
                     invited_hive_id, invited_operator_id, envelope_json,
                     one_time_secret, state, imported_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                         ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                         'keeper_pinned', ?19, ?20)",
                params![
                    invitation.invitation_id.to_string(),
                    invitation.apiary_id.to_string(),
                    &invitation.apiary_name,
                    invitation.shared_work_backend.to_string(),
                    invitation.required_policy_revision,
                    &invitation.promoted_project_catalog_digest,
                    invitation.keeper_node_id.to_string(),
                    invitation.keeper_hive_id.to_string(),
                    &invitation.keeper_hive_name,
                    invitation.keeper_operator_id.to_string(),
                    &invitation.keeper_operator_display_name,
                    &bundle.keeper_connection_card.payload.public_key,
                    &invitation.keeper_endpoint,
                    bundle.invitation.payload.invited_node_id.to_string(),
                    bundle.invitation.payload.invited_hive_id.to_string(),
                    bundle.invitation.payload.invited_operator_id.to_string(),
                    serde_json::to_string(&bundle.invitation)
                        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
                    secret.as_slice(),
                    now,
                    invitation.expires_at,
                ],
            )
            .map_err(map_join_invitation_insert_error)?;
        for project in &bundle.promoted_projects {
            transaction.execute(
                "INSERT INTO apiary_join_invitation_projects
                    (invitation_id, project_id, project_key, project_name)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    invitation.invitation_id.to_string(),
                    &project.project_id,
                    &project.project_key,
                    &project.project_name,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(invitation)
    }

    /// Lists current imported join invitations without exposing the retained
    /// bearer secret, Keeper public key, or complete signed envelope.
    ///
    /// # Errors
    /// Returns an error when durable invitation state is unavailable or corrupt.
    pub fn federation_join_invitations(
        &self,
        now: i64,
    ) -> Result<Vec<FederationJoinInvitation>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, apiary_id, apiary_name, shared_work_backend,
                    required_policy_revision, promoted_project_catalog_digest,
                    keeper_node_id, keeper_hive_id, keeper_hive_name,
                    keeper_operator_id, keeper_operator_display_name,
                    keeper_endpoint, state, imported_at, expires_at
             FROM apiary_join_invitations
             WHERE state IN ('keeper_pinned', 'policy_accepted', 'submitted')
               AND expires_at > ?1
             ORDER BY imported_at DESC, id",
        )?;
        let mut invitations = statement
            .query_map([now], federation_join_invitation_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)?;
        drop(statement);
        for invitation in &mut invitations {
            invitation.promoted_projects =
                imported_project_manifest(&connection, invitation.invitation_id)?;
        }
        Ok(invitations)
    }

    /// Returns local, private Jira evidence for every signed project in one
    /// imported invitation. Matching uses immutable Jira project identity,
    /// never display key or name.
    ///
    /// # Errors
    /// Rejects an invitation not addressed to this Hive and corrupt persistence.
    pub fn federation_project_readiness(
        &self,
        invitation_id: ApiaryInvitationId,
    ) -> Result<Vec<FederationProjectReadiness>, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let invited_hive_id = connection
            .query_row(
                "SELECT invited_hive_id FROM apiary_join_invitations WHERE id = ?1",
                [invitation_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)?;
        if invited_hive_id != identity.hive.id.to_string() {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let mut statement = connection.prepare(
            "SELECT p.project_id, p.project_key, p.project_name,
                    b.id, b.access_verified, b.workflow_mapped
             FROM apiary_join_invitation_projects p
             LEFT JOIN jira_project_bindings b
               ON b.hive_id = ?2 AND b.project_id = p.project_id
             WHERE p.invitation_id = ?1
             ORDER BY p.project_key COLLATE NOCASE, p.project_id",
        )?;
        statement
            .query_map(
                params![invitation_id.to_string(), identity.hive.id.to_string()],
                |row| {
                    Ok(FederationProjectReadiness {
                        project: FederationProjectManifestEntry {
                            project_id: row.get(0)?,
                            project_key: row.get(1)?,
                            project_name: row.get(2)?,
                        },
                        binding_id: row
                            .get::<_, Option<String>>(3)?
                            .as_deref()
                            .map(parse_domain_id::<JiraProjectBindingId>)
                            .transpose()?,
                        access_verified: row.get::<_, Option<bool>>(4)?.unwrap_or(false),
                        workflow_mapped: row.get::<_, Option<bool>>(5)?.unwrap_or(false),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Accepts only the exact policy revision carried by one current signed
    /// invitation. This transition grants no membership and performs no
    /// network request.
    ///
    /// # Errors
    /// Rejects stale revisions, expired/resolved invitations, non-personal
    /// Hives, identity mismatches, invalid time, and persistence failures.
    pub fn accept_federation_join_policy(
        &self,
        invitation_id: ApiaryInvitationId,
        policy_revision: u64,
        now: i64,
    ) -> Result<FederationJoinInvitation, TaskStoreError> {
        if now < 0 || policy_revision == 0 {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let identity = self.local_hive_identity()?;
        if identity.hive.apiary_id.is_some() {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE apiary_join_invitations
             SET state = 'policy_accepted', policy_accepted_at = ?1
             WHERE id = ?2
               AND invited_hive_id = ?3
               AND invited_operator_id = ?4
               AND state = 'keeper_pinned'
               AND required_policy_revision = ?5
               AND expires_at > ?1
               AND EXISTS (
                   SELECT 1 FROM hives h
                   WHERE h.id = apiary_join_invitations.invited_hive_id
                     AND h.apiary_id IS NULL
               )",
            params![
                now,
                invitation_id.to_string(),
                identity.hive.id.to_string(),
                identity.operator.id.to_string(),
                policy_revision,
            ],
        )?;
        drop(connection);
        if changed != 1 {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        self.federation_join_invitations(now)?
            .into_iter()
            .find(|invitation| invitation.invitation_id == invitation_id)
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)
    }

    /// Seals the locally derived readiness into a signed, retry-stable
    /// submission for the exact imported invitation. The retained bearer
    /// secret is included only in this private transport object.
    ///
    /// # Errors
    /// Rejects incomplete readiness, stale invitation state, identity drift,
    /// expiry, invalid time, and persistence failures.
    pub fn prepare_federation_join_submission(
        &self,
        invitation_id: ApiaryInvitationId,
        readiness: &FederationJoinReadiness,
        now: i64,
    ) -> Result<FederationJoinSubmission, TaskStoreError> {
        if now < 0 || !readiness.can_submit() {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let identity = self.local_hive_identity()?;
        if identity.hive.apiary_id.is_some() {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let local_node = self.local_federation_identity(now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let stored = transaction
            .query_row(
                "SELECT apiary_id, required_policy_revision,
                        promoted_project_catalog_digest, invited_node_id,
                        invited_hive_id, invited_operator_id, one_time_secret,
                        state, expires_at, submission_json
                 FROM apiary_join_invitations WHERE id = ?1",
                [invitation_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryInvitationNotFound)?;
        if let Some(json) = stored.9 {
            return serde_json::from_str(&json)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()));
        }
        if stored.7 != "policy_accepted"
            || stored.8 <= now
            || stored.3 != local_node.node_id.to_string()
            || stored.4 != identity.hive.id.to_string()
            || stored.5 != identity.operator.id.to_string()
            || stored.6.len() != 32
        {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let payload = FederationJoinSubmissionPayload {
            schema_version: FEDERATION_MEMBERSHIP_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            invitation_id,
            apiary_id: parse_domain_id(&stored.0)?,
            required_policy_revision: stored.1,
            promoted_project_catalog_digest: stored.2,
            invited_node_id: local_node.node_id,
            invited_hive_id: identity.hive.id,
            invited_operator_id: identity.operator.id,
            submitted_at: now,
        };
        let signature = local_node
            .signing_key
            .sign(&canonical_join_submission_payload(&payload)?);
        let submission = FederationJoinSubmission {
            payload,
            signature: Base64UrlUnpadded::encode_string(&signature.to_bytes()),
            one_time_secret: Base64UrlUnpadded::encode_string(&stored.6),
        };
        let serialized = serde_json::to_string(&submission)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        if transaction.execute(
            "UPDATE apiary_join_invitations
             SET state = 'submitted', submitted_at = ?1, submission_json = ?2
             WHERE id = ?3 AND state = 'policy_accepted' AND expires_at > ?1",
            params![now, serialized, invitation_id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        transaction.commit()?;
        Ok(submission)
    }

    /// Atomically consumes a Keeper-side one-time invitation and registers the
    /// pinned Hive as a member. Exact authenticated retries return the same
    /// receipt and credential so a lost response cannot create a second join.
    ///
    /// # Errors
    /// Rejects invalid signatures or secrets, identity/catalog/policy drift,
    /// expiry, replay with altered material, and membership conflicts.
    pub fn consume_federation_join_submission(
        &self,
        submission: &FederationJoinSubmission,
        now: i64,
    ) -> Result<FederationJoinAcceptance, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationInvitation);
        }
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let local_node = self.local_federation_identity(now)?;
        let secret: [u8; 32] = Base64UrlUnpadded::decode_vec(&submission.one_time_secret)
            .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let join = keeper_join_context(&transaction, submission.payload.invitation_id)?;
        verify_join_submission(submission, &join.public_key, now)?;
        let envelope: ApiaryInvitationEnvelope = serde_json::from_str(&join.envelope_json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let payload = &submission.payload;
        let bindings_match = join.apiary_id == apiary_id.to_string()
            && join.candidate_hive_id == payload.invited_hive_id.to_string()
            && join.candidate_node_id == payload.invited_node_id.to_string()
            && join.candidate_operator_id == payload.invited_operator_id.to_string()
            && payload.apiary_id == apiary_id
            && payload.required_policy_revision == envelope.payload.required_policy_revision
            && payload.promoted_project_catalog_digest
                == envelope.payload.promoted_project_catalog_digest
            && join.expires_at > now
            && constant_time_bytes_eq(&join.secret_digest, &invitation_secret_digest(&secret));
        if !bindings_match {
            return Err(TaskStoreError::InvalidFederationInvitation);
        }
        if join.state == "consumed" {
            return federation_acceptance_by_invitation(
                &transaction,
                submission.payload.invitation_id,
            )?
            .ok_or(TaskStoreError::IntegrityFailure(
                "consumed invitation has no membership receipt".into(),
            ));
        }
        if join.state != "pending" {
            return Err(TaskStoreError::ApiaryInvitationResolved);
        }
        let context =
            keeper_invitation_context(&transaction, apiary_id, &identity.operator.id.to_string())?;
        if context.policy_revision != payload.required_policy_revision
            || context.backend != SharedWorkBackend::Jira
            || promoted_project_manifest_digest(&promoted_project_manifest(
                &transaction,
                apiary_id,
            )?)? != payload.promoted_project_catalog_digest
        {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        let acceptance = register_federation_membership(
            &transaction,
            &identity,
            &local_node,
            &join,
            payload,
            context,
            now,
        )?;
        transaction.commit()?;
        Ok(acceptance)
    }

    /// Verifies and atomically applies the Keeper's signed acceptance on the
    /// invited Hive. Membership becomes visible only after the receipt and
    /// bounded credential are durable in the same transaction.
    ///
    /// # Errors
    /// Rejects invalid signatures, mismatched identities/policy/catalog,
    /// expired credentials, unsolicited receipts, and conflicting membership.
    pub fn apply_federation_join_acceptance(
        &self,
        invitation_id: ApiaryInvitationId,
        acceptance: &FederationJoinAcceptance,
        now: i64,
    ) -> Result<swarm_domain::LocalApiaryContext, TaskStoreError> {
        if now < 0 || acceptance.receipt.payload.invitation_id != invitation_id {
            return Err(TaskStoreError::InvalidFederationInvitation);
        }
        let credential: [u8; 32] = Base64UrlUnpadded::decode_vec(&acceptance.node_credential)
            .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let invitation = invited_join_application_context(&transaction, invitation_id)?;
        verify_federation_membership_receipt(
            &acceptance.receipt,
            &invitation.keeper_public_key,
            now,
        )?;
        validate_join_acceptance_bindings(&identity, &invitation, acceptance)?;
        if invitation.state == "consumed" {
            validate_stored_local_acceptance(&transaction, acceptance, &credential)?;
            drop(transaction);
            drop(connection);
            return self.local_apiary_context();
        }
        if invitation.state != "submitted" || identity.hive.apiary_id.is_some() {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        insert_remote_apiary_identity(&transaction, &invitation, now)?;
        if transaction.execute(
            "UPDATE hives SET apiary_id = ?1, updated_at = ?2
             WHERE id = ?3 AND apiary_id IS NULL",
            params![invitation.apiary_id, now, identity.hive.id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryMembershipConflict);
        }
        transaction.execute(
            "INSERT INTO local_federation_membership
                (singleton, receipt_id, invitation_id, apiary_id,
                 keeper_node_id, receipt_json, node_credential,
                 credential_digest, joined_at, credential_expires_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                acceptance.receipt.payload.receipt_id.to_string(),
                invitation_id.to_string(),
                invitation.apiary_id,
                invitation.keeper_node_id,
                serde_json::to_string(&acceptance.receipt)
                    .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
                credential.as_slice(),
                invitation_secret_digest(&credential).as_slice(),
                acceptance.receipt.payload.joined_at,
                acceptance.receipt.payload.credential_expires_at,
            ],
        )?;
        if transaction.execute(
            "UPDATE apiary_join_invitations
             SET state = 'consumed', resolved_at = ?1
             WHERE id = ?2 AND state = 'submitted'",
            params![now, invitation_id.to_string()],
        )? != 1
        {
            return Err(TaskStoreError::ApiaryJoinNotReady);
        }
        transaction.execute(
            "UPDATE apiary_join_invitations
             SET state = 'revoked', resolved_at = ?1
             WHERE id <> ?2
               AND state IN ('keeper_pinned', 'policy_accepted', 'submitted')",
            params![now, invitation_id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.local_apiary_context()
    }

    /// Reports the number of current distributed invitations for one pinned
    /// candidate without exposing their secret digests or envelopes.
    ///
    /// # Errors
    /// Rejects non-Keepers and unavailable persistence.
    pub fn pending_federation_invitation_count(
        &self,
        invited_hive_id: HiveId,
        now: i64,
    ) -> Result<usize, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let connection = self.connection()?;
        let keeper = connection
            .query_row(
                "SELECT keeper_operator_id FROM apiaries
                 WHERE id = ?1 AND collapsed_at IS NULL",
                [apiary_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        if keeper != identity.operator.id.to_string() {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        }
        let count = connection.query_row(
            "SELECT COUNT(*) FROM apiary_federation_invitations
             WHERE apiary_id = ?1 AND candidate_hive_id = ?2
               AND state = 'pending' AND expires_at > ?3",
            params![apiary_id.to_string(), invited_hive_id.to_string(), now],
            |row| row.get::<_, usize>(0),
        )?;
        Ok(count)
    }

    /// Verifies and pins one remote Hive identity for the current Keeper's
    /// active Apiary. Re-importing a newer card refreshes display metadata only
    /// when the node, Hive, operator, and public key remain identical.
    ///
    /// # Errors
    /// Rejects invalid or expired cards, non-Keepers, self-pinning, and every
    /// attempt to replace an already pinned identity key.
    pub fn pin_hive_candidate(
        &self,
        card: &HiveConnectionCard,
        now: i64,
    ) -> Result<ApiaryHiveCandidate, TaskStoreError> {
        verify_hive_connection_card(card, now)?;
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        if card.payload.hive_id == identity.hive.id
            || card.payload.operator_id == identity.operator.id
        {
            return Err(TaskStoreError::InvalidFederationConnectionCard);
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let keeper = transaction
            .query_row(
                "SELECT keeper_operator_id FROM apiaries
                 WHERE id = ?1 AND collapsed_at IS NULL",
                [apiary_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        if keeper != identity.operator.id.to_string() {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        }

        let existing = candidate_by_hive(&transaction, apiary_id, card.payload.hive_id)?;
        if existing.as_ref().is_some_and(|candidate| {
            candidate.node_id != card.payload.node_id
                || candidate.operator_id != card.payload.operator_id
                || candidate.public_key != card.payload.public_key
        }) {
            return Err(TaskStoreError::HiveCandidateIdentityConflict);
        }

        transaction
            .execute(
                "INSERT INTO apiary_hive_candidates
                    (apiary_id, node_id, hive_id, hive_name, operator_id,
                     operator_display_name, public_key, card_issued_at,
                     card_expires_at, pinned_by_operator_id, pinned_at,
                     last_verified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
                 ON CONFLICT(apiary_id, hive_id) DO UPDATE SET
                     hive_name = excluded.hive_name,
                     operator_display_name = excluded.operator_display_name,
                     card_issued_at = excluded.card_issued_at,
                     card_expires_at = excluded.card_expires_at,
                     last_verified_at = excluded.last_verified_at",
                params![
                    apiary_id.to_string(),
                    card.payload.node_id.to_string(),
                    card.payload.hive_id.to_string(),
                    &card.payload.hive_name,
                    card.payload.operator_id.to_string(),
                    &card.payload.operator_display_name,
                    &card.payload.public_key,
                    card.payload.issued_at,
                    card.payload.expires_at,
                    identity.operator.id.to_string(),
                    now,
                ],
            )
            .map_err(map_candidate_insert_error)?;
        let candidate = candidate_by_hive(&transaction, apiary_id, card.payload.hive_id)?
            .ok_or(TaskStoreError::HiveCandidateIdentityConflict)?;
        transaction.commit()?;
        Ok(candidate)
    }

    /// Lists identities pinned by the current active Keeper. Candidates are
    /// deliberately separate from Apiary members and invitations.
    ///
    /// # Errors
    /// Rejects personal Hives, members, inactive Apiaries, and invalid state.
    pub fn list_hive_candidates(&self) -> Result<Vec<ApiaryHiveCandidate>, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        let connection = self.connection()?;
        let keeper = connection
            .query_row(
                "SELECT keeper_operator_id FROM apiaries
                 WHERE id = ?1 AND collapsed_at IS NULL",
                [apiary_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
        if keeper != identity.operator.id.to_string() {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        }
        let mut statement = connection.prepare(
            "SELECT apiary_id, node_id, hive_id, hive_name, operator_id,
                    operator_display_name, public_key, card_issued_at,
                    card_expires_at, pinned_by_operator_id, pinned_at,
                    last_verified_at
             FROM apiary_hive_candidates WHERE apiary_id = ?1
             ORDER BY hive_name COLLATE NOCASE, hive_id",
        )?;
        Ok(statement
            .query_map([apiary_id.to_string()], candidate_from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn local_federation_identity(
        &self,
        now: i64,
    ) -> Result<LocalFederationIdentity, TaskStoreError> {
        let connection = self.connection()?;
        let stored = connection
            .query_row(
                "SELECT node_id, signing_seed, public_key
                 FROM local_federation_identity WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((node_id, seed, public_key)) = stored {
            return reconstitute_identity(&node_id, &seed, &public_key);
        }

        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
        let signing_key = SigningKey::from_bytes(&seed);
        let node_id = FederationNodeId::new();
        connection.execute(
            "INSERT INTO local_federation_identity
                (singleton, node_id, signing_seed, public_key, created_at)
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![
                node_id.to_string(),
                seed.as_slice(),
                signing_key.verifying_key().as_bytes().as_slice(),
                now
            ],
        )?;
        Ok(LocalFederationIdentity {
            node_id,
            signing_key,
        })
    }
}

fn connection_card_for(
    identity: &swarm_domain::HiveIdentity,
    local_node: &LocalFederationIdentity,
    now: i64,
    lifetime_seconds: i64,
) -> Result<HiveConnectionCard, TaskStoreError> {
    let payload = HiveConnectionCardPayload {
        schema_version: FEDERATION_CONNECTION_CARD_SCHEMA_VERSION,
        protocol_version: FEDERATION_PROTOCOL_VERSION,
        node_id: local_node.node_id,
        hive_id: identity.hive.id,
        hive_name: identity.hive.name.clone(),
        operator_id: identity.operator.id,
        operator_display_name: identity.operator.display_name.clone(),
        public_key: Base64UrlUnpadded::encode_string(
            local_node.signing_key.verifying_key().as_bytes(),
        ),
        issued_at: now,
        expires_at: now
            .checked_add(lifetime_seconds)
            .ok_or(TaskStoreError::InvalidFederationConnectionCard)?,
    };
    let signature = local_node.signing_key.sign(&canonical_payload(&payload)?);
    Ok(HiveConnectionCard {
        payload,
        signature: Base64UrlUnpadded::encode_string(&signature.to_bytes()),
    })
}

fn validate_invitation_endpoint(endpoint: &str) -> Result<(), TaskStoreError> {
    let endpoint = endpoint.trim();
    let parsed = Url::parse(endpoint).map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let local_http = parsed.scheme() == "http"
        && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if endpoint.is_empty()
        || endpoint.len() > MAX_KEEPER_ENDPOINT_BYTES
        || (parsed.scheme() != "https" && !local_http)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.host_str().is_none()
    {
        return Err(TaskStoreError::InvalidFederationInvitation);
    }
    Ok(())
}

fn invitation_material() -> Result<([u8; 32], String, String), TaskStoreError> {
    let (secret, one_time_secret) = bearer_secret_material()?;
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut nonce).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
    let nonce = Base64UrlUnpadded::encode_string(&nonce);
    Ok((secret, one_time_secret, nonce))
}

fn bearer_secret_material() -> Result<([u8; 32], String), TaskStoreError> {
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
    let encoded = Base64UrlUnpadded::encode_string(&secret);
    Ok((secret, encoded))
}

fn node_credential_material() -> Result<([u8; 32], String), TaskStoreError> {
    let mut credential = [0_u8; 32];
    getrandom::fill(&mut credential).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
    let encoded = Base64UrlUnpadded::encode_string(&credential);
    Ok((credential, encoded))
}

fn canonical_join_submission_payload(
    payload: &FederationJoinSubmissionPayload,
) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

fn canonical_membership_receipt_payload(
    payload: &FederationMembershipReceiptPayload,
) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

fn canonical_catalog_snapshot_payload(
    payload: &FederationCatalogSnapshotPayload,
) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

fn catalog_acknowledgement(
    snapshot: &FederationCatalogSnapshot,
    acknowledged_at: i64,
) -> FederationCatalogAcknowledgement {
    FederationCatalogAcknowledgement {
        apiary_id: snapshot.payload.apiary_id,
        policy_revision: snapshot.payload.policy_revision,
        promoted_project_catalog_digest: snapshot.payload.promoted_project_catalog_digest.clone(),
        project_count: snapshot.payload.projects.len(),
        snapshot_issued_at: snapshot.payload.issued_at,
        snapshot_expires_at: snapshot.payload.expires_at,
        acknowledged_at,
    }
}

pub(crate) fn decode_node_credential(value: &str) -> Result<[u8; 32], TaskStoreError> {
    Base64UrlUnpadded::decode_vec(value)
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationCredential)
}

fn validate_claim_identity(
    project_id: &str,
    issue_id: &str,
    issue_key: &str,
    now: i64,
) -> Result<(), TaskStoreError> {
    let values = [project_id.trim(), issue_id.trim(), issue_key.trim()];
    if now < 0
        || values.iter().any(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        })
    {
        return Err(TaskStoreError::InvalidFederationClaim);
    }
    Ok(())
}

pub(crate) fn authenticate_member_credential(
    connection: &rusqlite::Connection,
    identity: &swarm_domain::HiveIdentity,
    credential: &[u8; 32],
    now: i64,
) -> Result<MemberCredentialContext, TaskStoreError> {
    let apiary_id = identity
        .hive
        .apiary_id
        .ok_or(TaskStoreError::InvalidFederationCredential)?;
    let keeper = connection
        .query_row(
            "SELECT keeper_operator_id FROM apiaries
             WHERE id = ?1 AND collapsed_at IS NULL",
            [apiary_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(TaskStoreError::InvalidFederationCredential)?;
    if keeper != identity.operator.id.to_string() {
        return Err(TaskStoreError::InvalidFederationCredential);
    }
    let digest = invitation_secret_digest(credential);
    connection
        .query_row(
            "SELECT member_node_id, member_hive_id, member_operator_id
             FROM apiary_federation_memberships
             WHERE apiary_id = ?1 AND credential_digest = ?2
               AND credential_expires_at > ?3",
            params![apiary_id.to_string(), digest.as_slice(), now],
            |row| {
                Ok(MemberCredentialContext {
                    apiary: apiary_id,
                    node: parse_domain_id(&row.get::<_, String>(0)?)?,
                    hive: parse_domain_id(&row.get::<_, String>(1)?)?,
                    operator: parse_domain_id(&row.get::<_, String>(2)?)?,
                })
            },
        )
        .optional()?
        .ok_or(TaskStoreError::InvalidFederationCredential)
}

fn insert_federation_claim(
    connection: &rusqlite::Connection,
    claim: &FederationSharedClaim,
    now: i64,
) -> Result<(), TaskStoreError> {
    connection
        .execute(
            "INSERT INTO apiary_federation_claims
            (id, apiary_id, project_id, issue_id, issue_key,
             home_node_id, home_hive_id, home_operator_id, state,
             reserved_at, reservation_expires_at, confirmed_at,
             released_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, NULL, ?12)",
            params![
                claim.id.to_string(),
                claim.apiary_id.to_string(),
                &claim.project_id,
                &claim.issue_id,
                &claim.issue_key,
                claim.home_node_id.to_string(),
                claim.home_hive_id.to_string(),
                claim.home_operator_id.to_string(),
                claim.state.to_string(),
                claim.reserved_at,
                claim.reservation_expires_at,
                now,
            ],
        )
        .map_err(map_federation_claim_insert_error)?;
    Ok(())
}

fn active_claim_for_issue(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
    project_id: &str,
    issue_id: &str,
) -> Result<Option<FederationSharedClaim>, TaskStoreError> {
    connection
        .query_row(
            "SELECT id, apiary_id, project_id, issue_id, issue_key,
                    home_node_id, home_hive_id, home_operator_id, state,
                    reserved_at, reservation_expires_at, confirmed_at, released_at
             FROM apiary_federation_claims
             WHERE apiary_id = ?1 AND project_id = ?2 AND issue_id = ?3
               AND state IN ('reserved','confirmed')",
            params![apiary_id.to_string(), project_id, issue_id],
            federation_claim_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn federation_claim_by_id(
    connection: &rusqlite::Connection,
    claim_id: FederationClaimId,
) -> Result<Option<FederationSharedClaim>, TaskStoreError> {
    connection
        .query_row(
            "SELECT id, apiary_id, project_id, issue_id, issue_key,
                    home_node_id, home_hive_id, home_operator_id, state,
                    reserved_at, reservation_expires_at, confirmed_at, released_at
             FROM apiary_federation_claims WHERE id = ?1",
            [claim_id.to_string()],
            federation_claim_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn federation_claim_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FederationSharedClaim> {
    Ok(FederationSharedClaim {
        id: parse_domain_id(&row.get::<_, String>(0)?)?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        project_id: row.get(2)?,
        issue_id: row.get(3)?,
        issue_key: row.get(4)?,
        home_node_id: parse_domain_id(&row.get::<_, String>(5)?)?,
        home_hive_id: parse_domain_id(&row.get::<_, String>(6)?)?,
        home_operator_id: parse_domain_id(&row.get::<_, String>(7)?)?,
        state: row
            .get::<_, String>(8)?
            .parse()
            .map_err(|()| rusqlite::Error::InvalidQuery)?,
        reserved_at: row.get(9)?,
        reservation_expires_at: row.get(10)?,
        confirmed_at: row.get(11)?,
        released_at: row.get(12)?,
    })
}

fn verify_join_submission(
    submission: &FederationJoinSubmission,
    expected_public_key: &str,
    now: i64,
) -> Result<(), TaskStoreError> {
    let payload = &submission.payload;
    if payload.schema_version != FEDERATION_MEMBERSHIP_SCHEMA_VERSION
        || payload.protocol_version != FEDERATION_PROTOCOL_VERSION
        || payload.required_policy_revision == 0
        || payload.submitted_at < 0
        || payload.submitted_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS)
    {
        return Err(TaskStoreError::InvalidFederationInvitation);
    }
    let public_key: [u8; 32] = Base64UrlUnpadded::decode_vec(expected_public_key)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&submission.signature)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let canonical = canonical_join_submission_payload(payload)?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)
}

fn constant_time_bytes_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn federation_acceptance_by_invitation(
    connection: &rusqlite::Connection,
    invitation_id: ApiaryInvitationId,
) -> Result<Option<FederationJoinAcceptance>, TaskStoreError> {
    connection
        .query_row(
            "SELECT receipt_json, node_credential
             FROM apiary_federation_memberships WHERE invitation_id = ?1",
            [invitation_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?
        .map(|(receipt_json, credential)| {
            let receipt = serde_json::from_str(&receipt_json)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            if credential.len() != 32 {
                return Err(TaskStoreError::IntegrityFailure(
                    "stored federation node credential is invalid".into(),
                ));
            }
            Ok(FederationJoinAcceptance {
                receipt,
                node_credential: Base64UrlUnpadded::encode_string(&credential),
            })
        })
        .transpose()
}

fn keeper_join_context(
    connection: &rusqlite::Connection,
    invitation_id: ApiaryInvitationId,
) -> Result<KeeperJoinContext, TaskStoreError> {
    connection
        .query_row(
            "SELECT i.apiary_id, i.candidate_hive_id, i.candidate_node_id,
                    i.candidate_operator_id, i.secret_digest, i.state,
                    i.expires_at, c.hive_name, c.operator_display_name,
                    c.public_key, i.envelope_json
             FROM apiary_federation_invitations i
             JOIN apiary_hive_candidates c
               ON c.apiary_id = i.apiary_id
              AND c.hive_id = i.candidate_hive_id
             WHERE i.id = ?1",
            [invitation_id.to_string()],
            |row| {
                Ok(KeeperJoinContext {
                    apiary_id: row.get(0)?,
                    candidate_hive_id: row.get(1)?,
                    candidate_node_id: row.get(2)?,
                    candidate_operator_id: row.get(3)?,
                    secret_digest: row.get(4)?,
                    state: row.get(5)?,
                    expires_at: row.get(6)?,
                    hive_name: row.get(7)?,
                    operator_display_name: row.get(8)?,
                    public_key: row.get(9)?,
                    envelope_json: row.get(10)?,
                })
            },
        )
        .optional()?
        .ok_or(TaskStoreError::ApiaryInvitationNotFound)
}

fn register_federation_membership(
    transaction: &rusqlite::Transaction<'_>,
    identity: &swarm_domain::HiveIdentity,
    local_node: &LocalFederationIdentity,
    join: &KeeperJoinContext,
    payload: &FederationJoinSubmissionPayload,
    context: KeeperInvitationContext,
    now: i64,
) -> Result<FederationJoinAcceptance, TaskStoreError> {
    transaction
        .execute(
            "INSERT INTO operators (id, display_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![join.candidate_operator_id, join.operator_display_name, now],
        )
        .map_err(|_| TaskStoreError::ApiaryMembershipConflict)?;
    transaction
        .execute(
            "INSERT INTO hives (id, name, operator_id, apiary_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                join.candidate_hive_id,
                join.hive_name,
                join.candidate_operator_id,
                payload.apiary_id.to_string(),
                now
            ],
        )
        .map_err(|_| TaskStoreError::ApiaryMembershipConflict)?;
    let (credential, encoded_credential) = node_credential_material()?;
    let credential_expires_at = now
        .checked_add(FEDERATION_NODE_CREDENTIAL_LIFETIME_SECONDS)
        .ok_or(TaskStoreError::InvalidFederationInvitation)?;
    let receipt_payload = FederationMembershipReceiptPayload {
        schema_version: FEDERATION_MEMBERSHIP_SCHEMA_VERSION,
        protocol_version: FEDERATION_PROTOCOL_VERSION,
        receipt_id: FederationMembershipReceiptId::new(),
        invitation_id: payload.invitation_id,
        apiary_id: payload.apiary_id,
        apiary_name: context.apiary_name,
        shared_work_backend: context.backend,
        policy_revision: context.policy_revision,
        promoted_project_catalog_digest: payload.promoted_project_catalog_digest.clone(),
        keeper_node_id: local_node.node_id,
        keeper_hive_id: identity.hive.id,
        keeper_operator_id: identity.operator.id,
        member_node_id: payload.invited_node_id,
        member_hive_id: payload.invited_hive_id,
        member_operator_id: payload.invited_operator_id,
        joined_at: now,
        credential_expires_at,
    };
    let receipt_signature = local_node
        .signing_key
        .sign(&canonical_membership_receipt_payload(&receipt_payload)?);
    let receipt = FederationMembershipReceipt {
        payload: receipt_payload,
        signature: Base64UrlUnpadded::encode_string(&receipt_signature.to_bytes()),
    };
    transaction.execute(
        "INSERT INTO apiary_federation_memberships
            (receipt_id, invitation_id, apiary_id, member_node_id,
             member_hive_id, member_operator_id, receipt_json,
             node_credential, credential_digest, joined_at,
             credential_expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            receipt.payload.receipt_id.to_string(),
            payload.invitation_id.to_string(),
            payload.apiary_id.to_string(),
            payload.invited_node_id.to_string(),
            payload.invited_hive_id.to_string(),
            payload.invited_operator_id.to_string(),
            serde_json::to_string(&receipt)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
            credential.as_slice(),
            invitation_secret_digest(&credential).as_slice(),
            now,
            credential_expires_at,
        ],
    )?;
    if transaction.execute(
        "UPDATE apiary_federation_invitations
         SET state = 'consumed', consumed_at = ?1
         WHERE id = ?2 AND state = 'pending'",
        params![now, payload.invitation_id.to_string()],
    )? != 1
    {
        return Err(TaskStoreError::ApiaryInvitationResolved);
    }
    Ok(FederationJoinAcceptance {
        receipt,
        node_credential: encoded_credential,
    })
}

fn invited_join_application_context(
    connection: &rusqlite::Connection,
    invitation_id: ApiaryInvitationId,
) -> Result<InvitedJoinApplicationContext, TaskStoreError> {
    connection
        .query_row(
            "SELECT apiary_id, apiary_name, shared_work_backend,
                    required_policy_revision, promoted_project_catalog_digest,
                    keeper_node_id, keeper_hive_id, keeper_hive_name,
                    keeper_operator_id, keeper_operator_display_name,
                    keeper_public_key, invited_node_id, invited_hive_id,
                    invited_operator_id, state
             FROM apiary_join_invitations WHERE id = ?1",
            [invitation_id.to_string()],
            |row| {
                Ok(InvitedJoinApplicationContext {
                    apiary_id: row.get(0)?,
                    apiary_name: row.get(1)?,
                    backend: row
                        .get::<_, String>(2)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    policy_revision: row.get(3)?,
                    catalog_digest: row.get(4)?,
                    keeper_node_id: row.get(5)?,
                    keeper_hive_id: row.get(6)?,
                    keeper_hive_name: row.get(7)?,
                    keeper_operator_id: row.get(8)?,
                    keeper_operator_display_name: row.get(9)?,
                    keeper_public_key: row.get(10)?,
                    invited_node_id: row.get(11)?,
                    invited_hive_id: row.get(12)?,
                    invited_operator_id: row.get(13)?,
                    state: row.get(14)?,
                })
            },
        )
        .optional()?
        .ok_or(TaskStoreError::ApiaryInvitationNotFound)
}

fn validate_join_acceptance_bindings(
    identity: &swarm_domain::HiveIdentity,
    invitation: &InvitedJoinApplicationContext,
    acceptance: &FederationJoinAcceptance,
) -> Result<(), TaskStoreError> {
    let receipt = &acceptance.receipt.payload;
    let valid = receipt.apiary_id.to_string() == invitation.apiary_id
        && receipt.apiary_name == invitation.apiary_name
        && receipt.shared_work_backend == invitation.backend
        && receipt.policy_revision == invitation.policy_revision
        && receipt.promoted_project_catalog_digest == invitation.catalog_digest
        && receipt.keeper_node_id.to_string() == invitation.keeper_node_id
        && receipt.keeper_hive_id.to_string() == invitation.keeper_hive_id
        && receipt.keeper_operator_id.to_string() == invitation.keeper_operator_id
        && receipt.member_node_id.to_string() == invitation.invited_node_id
        && receipt.member_hive_id == identity.hive.id
        && receipt.member_hive_id.to_string() == invitation.invited_hive_id
        && receipt.member_operator_id == identity.operator.id
        && receipt.member_operator_id.to_string() == invitation.invited_operator_id;
    if valid {
        Ok(())
    } else {
        Err(TaskStoreError::InvalidFederationInvitation)
    }
}

fn insert_remote_apiary_identity(
    transaction: &rusqlite::Transaction<'_>,
    invitation: &InvitedJoinApplicationContext,
    now: i64,
) -> Result<(), TaskStoreError> {
    transaction
        .execute(
            "INSERT INTO operators (id, display_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![
                invitation.keeper_operator_id,
                invitation.keeper_operator_display_name,
                now
            ],
        )
        .map_err(|_| TaskStoreError::ApiaryMembershipConflict)?;
    transaction
        .execute(
            "INSERT INTO apiaries
                (id, name, keeper_operator_id, shared_work_backend,
                 policy_revision, created_at, updated_at, collapsed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, NULL)",
            params![
                invitation.apiary_id,
                invitation.apiary_name,
                invitation.keeper_operator_id,
                invitation.backend.to_string(),
                invitation.policy_revision,
                now
            ],
        )
        .map_err(|_| TaskStoreError::ApiaryMembershipConflict)?;
    transaction
        .execute(
            "INSERT INTO hives (id, name, operator_id, apiary_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                invitation.keeper_hive_id,
                invitation.keeper_hive_name,
                invitation.keeper_operator_id,
                invitation.apiary_id,
                now
            ],
        )
        .map_err(|_| TaskStoreError::ApiaryMembershipConflict)?;
    Ok(())
}

fn validate_stored_local_acceptance(
    connection: &rusqlite::Connection,
    acceptance: &FederationJoinAcceptance,
    credential: &[u8; 32],
) -> Result<(), TaskStoreError> {
    let stored = connection
        .query_row(
            "SELECT receipt_json, node_credential
             FROM local_federation_membership WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            TaskStoreError::IntegrityFailure("consumed join has no local membership".into())
        })?;
    let receipt: FederationMembershipReceipt = serde_json::from_str(&stored.0)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    if receipt == acceptance.receipt && constant_time_bytes_eq(&stored.1, credential) {
        Ok(())
    } else {
        Err(TaskStoreError::InvalidFederationInvitation)
    }
}

fn keeper_invitation_context(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
    expected_keeper_operator_id: &str,
) -> Result<KeeperInvitationContext, TaskStoreError> {
    let (apiary_name, backend, policy_revision, keeper_operator_id) = connection
        .query_row(
            "SELECT name, shared_work_backend, policy_revision, keeper_operator_id
             FROM apiaries WHERE id = ?1 AND collapsed_at IS NULL",
            [apiary_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(TaskStoreError::ApiaryKeeperRequired)?;
    if keeper_operator_id != expected_keeper_operator_id {
        return Err(TaskStoreError::ApiaryKeeperRequired);
    }
    Ok(KeeperInvitationContext {
        apiary_name,
        backend: backend
            .parse::<SharedWorkBackend>()
            .map_err(|_| TaskStoreError::InvalidApiary)?,
        policy_revision,
    })
}

fn validate_imported_invitation_bindings(
    bundle: &ApiaryInvitationBundle,
    local_identity: &swarm_domain::HiveIdentity,
    local_node_id: FederationNodeId,
) -> Result<[u8; 32], TaskStoreError> {
    let keeper = &bundle.keeper_connection_card.payload;
    let invitation = &bundle.invitation.payload;
    let identities_match = invitation.keeper_node_id == keeper.node_id
        && invitation.keeper_hive_id == keeper.hive_id
        && invitation.keeper_operator_id == keeper.operator_id
        && invitation.invited_node_id == local_node_id
        && invitation.invited_hive_id == local_identity.hive.id
        && invitation.invited_operator_id == local_identity.operator.id
        && invitation.keeper_node_id != local_node_id
        && invitation.keeper_hive_id != local_identity.hive.id
        && invitation.keeper_operator_id != local_identity.operator.id;
    let manifest_valid = validate_project_manifest(&bundle.promoted_projects)
        && promoted_project_manifest_digest(&bundle.promoted_projects)
            .is_ok_and(|digest| digest == invitation.promoted_project_catalog_digest);
    if !identities_match
        || invitation.shared_work_backend != SharedWorkBackend::Jira
        || !manifest_valid
    {
        return Err(TaskStoreError::InvalidFederationInvitation);
    }
    Base64UrlUnpadded::decode_vec(&bundle.one_time_secret)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)
}

fn validate_project_manifest(projects: &[FederationProjectManifestEntry]) -> bool {
    if projects.len() > MAX_PROMOTED_PROJECTS_PER_INVITATION {
        return false;
    }
    let mut ids = HashSet::with_capacity(projects.len());
    let mut keys = HashSet::with_capacity(projects.len());
    projects.iter().all(|project| {
        let project_id = project.project_id.trim();
        let project_key = project.project_key.trim();
        let project_name = project.project_name.trim();
        project_id == project.project_id
            && project_key == project.project_key
            && project_name == project.project_name
            && !project_id.is_empty()
            && project_id.len() <= 128
            && !project_key.is_empty()
            && project_key.len() <= 64
            && !project_name.is_empty()
            && project_name.len() <= 256
            && ids.insert(project_id.to_owned())
            && keys.insert(project_key.to_ascii_uppercase())
    })
}

fn federation_join_invitation(
    bundle: &ApiaryInvitationBundle,
    imported_at: i64,
) -> FederationJoinInvitation {
    let keeper = &bundle.keeper_connection_card.payload;
    let invitation = &bundle.invitation.payload;
    FederationJoinInvitation {
        invitation_id: invitation.invitation_id,
        apiary_id: invitation.apiary_id,
        apiary_name: invitation.apiary_name.clone(),
        shared_work_backend: invitation.shared_work_backend,
        required_policy_revision: invitation.required_policy_revision,
        promoted_project_catalog_digest: invitation.promoted_project_catalog_digest.clone(),
        promoted_projects: bundle.promoted_projects.clone(),
        keeper_node_id: invitation.keeper_node_id,
        keeper_hive_id: invitation.keeper_hive_id,
        keeper_hive_name: keeper.hive_name.clone(),
        keeper_operator_id: invitation.keeper_operator_id,
        keeper_operator_display_name: keeper.operator_display_name.clone(),
        keeper_endpoint: invitation.keeper_endpoint.clone(),
        state: FederationJoinInvitationState::KeeperPinned,
        imported_at,
        expires_at: invitation.expires_at,
    }
}

fn invitation_secret_digest(secret: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.apiary-invitation-secret.v1\0");
    digest.update(secret);
    digest.finalize().into()
}

fn join_link_secret_digest(secret: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.apiary-join-link-secret.v1\0");
    digest.update(secret);
    digest.finalize().into()
}

fn promoted_project_manifest(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
) -> Result<Vec<FederationProjectManifestEntry>, TaskStoreError> {
    let mut statement = connection.prepare(
        "SELECT project_id, project_key, project_name
         FROM apiary_jira_projects WHERE apiary_id = ?1
         ORDER BY project_key COLLATE NOCASE, project_id",
    )?;
    statement
        .query_map([apiary_id.to_string()], |row| {
            Ok(FederationProjectManifestEntry {
                project_id: row.get(0)?,
                project_key: row.get(1)?,
                project_name: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn promoted_project_manifest_digest(
    projects: &[FederationProjectManifestEntry],
) -> Result<String, TaskStoreError> {
    let canonical = serde_json::to_vec(&projects)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.apiary-project-catalog.v1\0");
    digest.update(canonical);
    Ok(Base64UrlUnpadded::encode_string(&digest.finalize()))
}

fn imported_project_manifest(
    connection: &rusqlite::Connection,
    invitation_id: ApiaryInvitationId,
) -> Result<Vec<FederationProjectManifestEntry>, TaskStoreError> {
    let mut statement = connection.prepare(
        "SELECT project_id, project_key, project_name
         FROM apiary_join_invitation_projects
         WHERE invitation_id = ?1
         ORDER BY project_key COLLATE NOCASE, project_id",
    )?;
    statement
        .query_map([invitation_id.to_string()], |row| {
            Ok(FederationProjectManifestEntry {
                project_id: row.get(0)?,
                project_key: row.get(1)?,
                project_name: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Verifies a Keeper invitation against a public key already pinned by the
/// invited Hive. A key carried only inside an untrusted bundle is not enough.
///
/// # Errors
/// Rejects unsupported versions, malformed bounds or endpoint data, expired
/// envelopes, encoding failures, and invalid signatures.
pub fn verify_apiary_invitation_envelope(
    envelope: &ApiaryInvitationEnvelope,
    expected_keeper_public_key: &str,
    now: i64,
) -> Result<(), TaskStoreError> {
    let payload = &envelope.payload;
    if now < 0
        || payload.schema_version != FEDERATION_INVITATION_SCHEMA_VERSION
        || payload.protocol_version != FEDERATION_PROTOCOL_VERSION
        || payload.apiary_name.trim().is_empty()
        || payload.required_policy_revision == 0
        || payload.issued_at < 0
        || payload.issued_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS)
        || payload.expires_at <= now
        || payload.expires_at <= payload.issued_at
        || payload.expires_at - payload.issued_at > MAX_FEDERATION_INVITATION_LIFETIME_SECONDS
        || !Base64UrlUnpadded::decode_vec(&payload.promoted_project_catalog_digest)
            .is_ok_and(|digest| digest.len() == 32)
        || !Base64UrlUnpadded::decode_vec(&payload.nonce).is_ok_and(|nonce| nonce.len() == 24)
        || validate_invitation_endpoint(&payload.keeper_endpoint).is_err()
    {
        return Err(TaskStoreError::InvalidFederationInvitation);
    }
    let public_key: [u8; 32] = Base64UrlUnpadded::decode_vec(expected_keeper_public_key)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&envelope.signature)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let canonical = canonical_invitation_payload(payload)?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)
}

/// Verifies a membership receipt against the Keeper public key already pinned
/// by the invited Hive.
///
/// # Errors
/// Rejects unsupported versions, invalid bounds or encoding, identity
/// tampering, expired credentials, and invalid signatures.
pub fn verify_federation_membership_receipt(
    receipt: &FederationMembershipReceipt,
    expected_keeper_public_key: &str,
    now: i64,
) -> Result<(), TaskStoreError> {
    let payload = &receipt.payload;
    if now < 0
        || payload.schema_version != FEDERATION_MEMBERSHIP_SCHEMA_VERSION
        || payload.protocol_version != FEDERATION_PROTOCOL_VERSION
        || payload.apiary_name.trim().is_empty()
        || payload.policy_revision == 0
        || payload.joined_at < 0
        || payload.joined_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS)
        || payload.credential_expires_at <= now
        || payload.credential_expires_at <= payload.joined_at
        || payload.credential_expires_at - payload.joined_at
            > FEDERATION_NODE_CREDENTIAL_LIFETIME_SECONDS
    {
        return Err(TaskStoreError::InvalidFederationInvitation);
    }
    let public_key: [u8; 32] = Base64UrlUnpadded::decode_vec(expected_keeper_public_key)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&receipt.signature)
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)?;
    let canonical = canonical_membership_receipt_payload(payload)?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| TaskStoreError::InvalidFederationInvitation)
}

/// Verifies a short-lived catalog snapshot against the Keeper key pinned by
/// the member Hive and every federation identity in its membership receipt.
///
/// # Errors
/// Rejects unsupported versions, stale bounds, altered project catalogs,
/// recipient mismatch, malformed encoding, and invalid signatures.
pub fn verify_federation_catalog_snapshot(
    snapshot: &FederationCatalogSnapshot,
    expected_keeper_public_key: &str,
    membership_receipt: &FederationMembershipReceipt,
    now: i64,
) -> Result<(), TaskStoreError> {
    let payload = &snapshot.payload;
    let membership = &membership_receipt.payload;
    if now < 0
        || payload.schema_version != FEDERATION_CATALOG_SCHEMA_VERSION
        || payload.protocol_version != FEDERATION_PROTOCOL_VERSION
        || payload.policy_revision == 0
        || payload.apiary_id != membership.apiary_id
        || payload.keeper_node_id != membership.keeper_node_id
        || payload.keeper_hive_id != membership.keeper_hive_id
        || payload.keeper_operator_id != membership.keeper_operator_id
        || payload.member_node_id != membership.member_node_id
        || payload.issued_at < 0
        || payload.issued_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS)
        || payload.expires_at <= now
        || payload.expires_at <= payload.issued_at
        || payload.expires_at - payload.issued_at > FEDERATION_CATALOG_LIFETIME_SECONDS
        || promoted_project_manifest_digest(&payload.projects)?
            != payload.promoted_project_catalog_digest
    {
        return Err(TaskStoreError::InvalidFederationCredential);
    }
    let public_key: [u8; 32] = Base64UrlUnpadded::decode_vec(expected_keeper_public_key)
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&snapshot.signature)
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
    let canonical = canonical_catalog_snapshot_payload(payload)?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| TaskStoreError::InvalidFederationCredential)
}

fn canonical_invitation_payload(
    payload: &ApiaryInvitationEnvelopePayload,
) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

/// Verifies the card's canonical signature, versions, and bounded validity
/// window without trusting any identity asserted inside it.
///
/// # Errors
/// Returns an error for malformed encoding, unsupported versions, an invalid
/// signature, unreasonable timestamps, or an expired card.
pub fn verify_hive_connection_card(
    card: &HiveConnectionCard,
    now: i64,
) -> Result<(), TaskStoreError> {
    let payload = &card.payload;
    if now < 0
        || payload.schema_version != FEDERATION_CONNECTION_CARD_SCHEMA_VERSION
        || payload.protocol_version != FEDERATION_PROTOCOL_VERSION
        || payload.hive_name.trim().is_empty()
        || payload.operator_display_name.trim().is_empty()
        || payload.issued_at < 0
        || payload.issued_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS)
        || payload.expires_at <= now
        || payload.expires_at <= payload.issued_at
        || payload.expires_at - payload.issued_at > MAX_CONNECTION_CARD_LIFETIME_SECONDS
    {
        return Err(TaskStoreError::InvalidFederationConnectionCard);
    }
    let public_key: [u8; 32] = Base64UrlUnpadded::decode_vec(&payload.public_key)
        .map_err(|_| TaskStoreError::InvalidFederationConnectionCard)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationConnectionCard)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&card.signature)
        .map_err(|_| TaskStoreError::InvalidFederationConnectionCard)?
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationConnectionCard)?;
    let canonical = canonical_payload(payload)?;
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| TaskStoreError::InvalidFederationConnectionCard)
}

fn canonical_payload(payload: &HiveConnectionCardPayload) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

fn reconstitute_identity(
    node_id: &str,
    seed: &[u8],
    stored_public_key: &[u8],
) -> Result<LocalFederationIdentity, TaskStoreError> {
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| TaskStoreError::InvalidFederationIdentity)?;
    let signing_key = SigningKey::from_bytes(&seed);
    if signing_key.verifying_key().as_bytes() != stored_public_key {
        return Err(TaskStoreError::InvalidFederationIdentity);
    }
    Ok(LocalFederationIdentity {
        node_id: FederationNodeId::from_str(node_id)
            .map_err(|_| TaskStoreError::InvalidFederationIdentity)?,
        signing_key,
    })
}

pub(super) fn migrate_apiary_join_links(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_join_links (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             created_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             keeper_endpoint TEXT NOT NULL,
             secret_digest BLOB NOT NULL UNIQUE CHECK (length(secret_digest) = 32),
             state TEXT NOT NULL CHECK (state IN (
                 'open','awaiting_approval','approved','invitation_issued','revoked'
             )),
             candidate_hive_id TEXT,
             invitation_id TEXT REFERENCES apiary_federation_invitations(id),
             invitation_bundle_json TEXT,
             created_at INTEGER NOT NULL CHECK (created_at >= 0),
             expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
             approved_at INTEGER,
             FOREIGN KEY (apiary_id, candidate_hive_id)
                 REFERENCES apiary_hive_candidates(apiary_id, hive_id)
         );
         CREATE INDEX IF NOT EXISTS apiary_join_links_by_apiary
             ON apiary_join_links(apiary_id, created_at DESC);
         CREATE TRIGGER IF NOT EXISTS apiary_join_link_keeper_insert
             BEFORE INSERT ON apiary_join_links
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.collapsed_at IS NULL
                   AND a.keeper_operator_id = NEW.created_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only an active Keeper can create a join link'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_join_link_identity
             BEFORE UPDATE OF id, apiary_id, created_by_operator_id,
                              keeper_endpoint, secret_digest, created_at, expires_at
             ON apiary_join_links
             BEGIN SELECT RAISE(ABORT, 'Apiary join link identity is immutable'); END;
         CREATE TRIGGER IF NOT EXISTS bind_apiary_join_link_candidate_once
             BEFORE UPDATE OF candidate_hive_id ON apiary_join_links
             WHEN OLD.candidate_hive_id IS NOT NULL
               OR NEW.candidate_hive_id IS NULL
             BEGIN SELECT RAISE(ABORT, 'Apiary join link candidate is already bound'); END;
         PRAGMA user_version = 45;",
    )
}

pub(super) fn migrate_local_apiary_keeper_links(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_apiary_keeper_links (
             link_id TEXT PRIMARY KEY,
             keeper_endpoint TEXT NOT NULL,
             one_time_secret BLOB NOT NULL CHECK (length(one_time_secret) = 32),
             apiary_name TEXT,
             state TEXT NOT NULL CHECK (state IN (
                 'open','awaiting_approval','approved','invitation_issued','revoked','expired'
             )),
             created_at INTEGER NOT NULL CHECK (created_at >= 0),
             updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
             expires_at INTEGER
         );
         CREATE TRIGGER IF NOT EXISTS immutable_local_apiary_keeper_link
             BEFORE UPDATE OF link_id, keeper_endpoint, one_time_secret, created_at
             ON local_apiary_keeper_links
             BEGIN SELECT RAISE(ABORT, 'Local Apiary Keeper link identity is immutable'); END;
         PRAGMA user_version = 46;",
    )
}

pub(super) fn migrate_federation_identity(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_federation_identity (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             node_id TEXT NOT NULL UNIQUE,
             signing_seed BLOB NOT NULL CHECK (length(signing_seed) = 32),
             public_key BLOB NOT NULL UNIQUE CHECK (length(public_key) = 32),
             created_at INTEGER NOT NULL CHECK (created_at >= 0)
         );
         PRAGMA user_version = 30;",
    )
}

pub(super) fn migrate_federation_candidates(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_hive_candidates (
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             node_id TEXT NOT NULL,
             hive_id TEXT NOT NULL,
             hive_name TEXT NOT NULL,
             operator_id TEXT NOT NULL,
             operator_display_name TEXT NOT NULL,
             public_key TEXT NOT NULL,
             card_issued_at INTEGER NOT NULL,
             card_expires_at INTEGER NOT NULL CHECK (card_expires_at > card_issued_at),
             pinned_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             pinned_at INTEGER NOT NULL,
             last_verified_at INTEGER NOT NULL,
             PRIMARY KEY (apiary_id, hive_id),
             UNIQUE (apiary_id, node_id),
             UNIQUE (apiary_id, public_key)
         );
         CREATE TRIGGER IF NOT EXISTS apiary_hive_candidate_keeper_insert
             BEFORE INSERT ON apiary_hive_candidates
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.collapsed_at IS NULL
                   AND a.keeper_operator_id = NEW.pinned_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only an active Keeper can pin a Hive'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_hive_candidate_identity
             BEFORE UPDATE OF apiary_id, node_id, hive_id, operator_id,
                              public_key, pinned_by_operator_id, pinned_at
             ON apiary_hive_candidates
             BEGIN SELECT RAISE(ABORT, 'Pinned Hive identity is immutable'); END;
         PRAGMA user_version = 31;",
    )
}

pub(super) fn migrate_federation_invitations(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_federation_invitations (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             candidate_hive_id TEXT NOT NULL,
             candidate_node_id TEXT NOT NULL,
             candidate_operator_id TEXT NOT NULL,
             invited_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             secret_digest BLOB NOT NULL UNIQUE CHECK (length(secret_digest) = 32),
             nonce TEXT NOT NULL UNIQUE,
             envelope_json TEXT NOT NULL,
             state TEXT NOT NULL CHECK (state IN ('pending', 'consumed', 'revoked', 'expired')),
             created_at INTEGER NOT NULL CHECK (created_at >= 0),
             expires_at INTEGER NOT NULL CHECK (expires_at > created_at),
             consumed_at INTEGER,
             FOREIGN KEY (apiary_id, candidate_hive_id)
                 REFERENCES apiary_hive_candidates(apiary_id, hive_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_pending_federation_invitation_per_candidate
             ON apiary_federation_invitations(apiary_id, candidate_hive_id)
             WHERE state = 'pending';
         CREATE TRIGGER IF NOT EXISTS apiary_federation_invitation_keeper_insert
             BEFORE INSERT ON apiary_federation_invitations
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.collapsed_at IS NULL
                   AND a.keeper_operator_id = NEW.invited_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only an active Keeper can invite a Hive'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_federation_invitation_identity
             BEFORE UPDATE OF id, apiary_id, candidate_hive_id,
                              candidate_node_id, candidate_operator_id,
                              invited_by_operator_id, secret_digest, nonce,
                              envelope_json, created_at, expires_at
             ON apiary_federation_invitations
             BEGIN SELECT RAISE(ABORT, 'Federation invitation identity is immutable'); END;
         PRAGMA user_version = 32;",
    )
}

pub(super) fn migrate_federation_join_invitations(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_join_invitations (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL,
             apiary_name TEXT NOT NULL,
             shared_work_backend TEXT NOT NULL CHECK (shared_work_backend IN ('jira','native')),
             required_policy_revision INTEGER NOT NULL CHECK (required_policy_revision > 0),
             promoted_project_catalog_digest TEXT NOT NULL,
             keeper_node_id TEXT NOT NULL,
             keeper_hive_id TEXT NOT NULL,
             keeper_hive_name TEXT NOT NULL,
             keeper_operator_id TEXT NOT NULL,
             keeper_operator_display_name TEXT NOT NULL,
             keeper_public_key TEXT NOT NULL,
             keeper_endpoint TEXT NOT NULL,
             invited_node_id TEXT NOT NULL,
             invited_hive_id TEXT NOT NULL,
             invited_operator_id TEXT NOT NULL,
             envelope_json TEXT NOT NULL,
             one_time_secret BLOB NOT NULL CHECK (length(one_time_secret) = 32),
             state TEXT NOT NULL CHECK (
                 state IN ('keeper_pinned', 'policy_accepted', 'submitted',
                           'consumed', 'revoked', 'expired')
             ),
             imported_at INTEGER NOT NULL CHECK (imported_at >= 0),
             expires_at INTEGER NOT NULL CHECK (expires_at > imported_at),
             policy_accepted_at INTEGER,
             submitted_at INTEGER,
             resolved_at INTEGER
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_current_join_invitation_per_apiary
             ON apiary_join_invitations(apiary_id)
             WHERE state IN ('keeper_pinned', 'policy_accepted', 'submitted');
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_join_invitation_identity
             BEFORE UPDATE OF id, apiary_id, shared_work_backend,
                              required_policy_revision,
                              promoted_project_catalog_digest, keeper_node_id,
                              keeper_hive_id, keeper_operator_id,
                              keeper_public_key, keeper_endpoint,
                              invited_node_id, invited_hive_id,
                              invited_operator_id, envelope_json,
                              one_time_secret, imported_at, expires_at
             ON apiary_join_invitations
             BEGIN SELECT RAISE(ABORT, 'Imported Apiary invitation identity is immutable'); END;
         PRAGMA user_version = 33;",
    )
}

pub(super) fn migrate_federation_join_invitation_projects(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_join_invitation_projects (
             invitation_id TEXT NOT NULL
                 REFERENCES apiary_join_invitations(id) ON DELETE CASCADE,
             project_id TEXT NOT NULL,
             project_key TEXT NOT NULL COLLATE NOCASE,
             project_name TEXT NOT NULL,
             PRIMARY KEY (invitation_id, project_id),
             UNIQUE (invitation_id, project_key)
         );
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_join_invitation_projects
             BEFORE UPDATE ON apiary_join_invitation_projects
             BEGIN SELECT RAISE(ABORT, 'Imported project manifest is immutable'); END;
         PRAGMA user_version = 34;",
    )
}

pub(super) fn migrate_federation_memberships(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let has_submission_json = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM pragma_table_info('apiary_join_invitations')
             WHERE name = 'submission_json'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_submission_json {
        transaction.execute(
            "ALTER TABLE apiary_join_invitations ADD COLUMN submission_json TEXT",
            [],
        )?;
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_federation_memberships (
             receipt_id TEXT PRIMARY KEY,
             invitation_id TEXT NOT NULL UNIQUE
                 REFERENCES apiary_federation_invitations(id),
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             member_node_id TEXT NOT NULL,
             member_hive_id TEXT NOT NULL UNIQUE REFERENCES hives(id),
             member_operator_id TEXT NOT NULL UNIQUE REFERENCES operators(id),
             receipt_json TEXT NOT NULL,
             node_credential BLOB NOT NULL CHECK (length(node_credential) = 32),
             credential_digest BLOB NOT NULL UNIQUE CHECK (length(credential_digest) = 32),
             joined_at INTEGER NOT NULL CHECK (joined_at >= 0),
             credential_expires_at INTEGER NOT NULL CHECK (credential_expires_at > joined_at),
             UNIQUE (apiary_id, member_node_id)
         );
         CREATE TRIGGER IF NOT EXISTS immutable_apiary_federation_membership
             BEFORE UPDATE OF receipt_id, invitation_id, apiary_id,
                              member_node_id, member_hive_id, member_operator_id,
                              receipt_json, joined_at
             ON apiary_federation_memberships
             BEGIN SELECT RAISE(ABORT, 'Federation membership identity is immutable'); END;
         PRAGMA user_version = 35;",
    )
}

pub(super) fn migrate_local_federation_membership(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_federation_membership (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             receipt_id TEXT NOT NULL UNIQUE,
             invitation_id TEXT NOT NULL UNIQUE
                 REFERENCES apiary_join_invitations(id),
             apiary_id TEXT NOT NULL UNIQUE REFERENCES apiaries(id),
             keeper_node_id TEXT NOT NULL,
             receipt_json TEXT NOT NULL,
             node_credential BLOB NOT NULL CHECK (length(node_credential) = 32),
             credential_digest BLOB NOT NULL UNIQUE CHECK (length(credential_digest) = 32),
             joined_at INTEGER NOT NULL CHECK (joined_at >= 0),
             credential_expires_at INTEGER NOT NULL CHECK (credential_expires_at > joined_at)
         );
         CREATE TRIGGER IF NOT EXISTS immutable_local_federation_membership
             BEFORE UPDATE ON local_federation_membership
             BEGIN SELECT RAISE(ABORT, 'Local federation membership is immutable'); END;
         PRAGMA user_version = 36;",
    )
}

pub(super) fn migrate_local_federation_catalog(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_federation_catalog (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
             catalog_digest TEXT NOT NULL,
             project_count INTEGER NOT NULL CHECK (project_count >= 0),
             snapshot_json TEXT NOT NULL,
             snapshot_issued_at INTEGER NOT NULL CHECK (snapshot_issued_at >= 0),
             snapshot_expires_at INTEGER NOT NULL
                 CHECK (snapshot_expires_at > snapshot_issued_at),
             acknowledged_at INTEGER NOT NULL CHECK (acknowledged_at >= 0)
         );
         PRAGMA user_version = 37;",
    )
}

pub(super) fn migrate_federation_claims(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_federation_claims (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             project_id TEXT NOT NULL,
             issue_id TEXT NOT NULL,
             issue_key TEXT NOT NULL,
             home_node_id TEXT NOT NULL,
             home_hive_id TEXT NOT NULL REFERENCES hives(id),
             home_operator_id TEXT NOT NULL REFERENCES operators(id),
             state TEXT NOT NULL
                 CHECK (state IN ('reserved','confirmed','released','expired')),
             reserved_at INTEGER NOT NULL CHECK (reserved_at >= 0),
             reservation_expires_at INTEGER NOT NULL
                 CHECK (reservation_expires_at > reserved_at),
             confirmed_at INTEGER,
             released_at INTEGER,
             updated_at INTEGER NOT NULL CHECK (updated_at >= reserved_at),
             FOREIGN KEY (apiary_id, project_id)
                 REFERENCES apiary_jira_projects(apiary_id, project_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_active_apiary_claim_per_issue
             ON apiary_federation_claims(apiary_id, project_id, issue_id)
             WHERE state IN ('reserved','confirmed');
         CREATE INDEX IF NOT EXISTS apiary_claims_by_home_hive
             ON apiary_federation_claims(apiary_id, home_hive_id, state);
         PRAGMA user_version = 38;",
    )
}

pub(super) fn migrate_local_federation_sync(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_federation_sync (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             condition TEXT NOT NULL CHECK (condition IN
                 ('idle','current','offline','authentication_required','incompatible')),
             last_attempt_at INTEGER CHECK (last_attempt_at >= 0),
             last_success_at INTEGER CHECK (last_success_at >= 0),
             consecutive_failures INTEGER NOT NULL DEFAULT 0
                 CHECK (consecutive_failures >= 0 AND consecutive_failures <= 1000),
             next_attempt_at INTEGER CHECK (next_attempt_at >= 0),
             updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
         );
         PRAGMA user_version = 39;",
    )
}

fn federation_sync_health_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationSyncHealth> {
    let condition = row
        .get::<_, String>(0)?
        .parse::<FederationSyncCondition>()
        .map_err(|()| rusqlite::Error::InvalidQuery)?;
    let failures = row.get::<_, i64>(3)?;
    let consecutive_failures = u32::try_from(failures)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, failures))?;
    Ok(FederationSyncHealth {
        condition,
        last_attempt_at: row.get(1)?,
        last_success_at: row.get(2)?,
        consecutive_failures,
        next_attempt_at: row.get(4)?,
    })
}

fn map_candidate_insert_error(error: rusqlite::Error) -> TaskStoreError {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("UNIQUE constraint failed") =>
        {
            TaskStoreError::HiveCandidateIdentityConflict
        }
        other => TaskStoreError::Sql(other),
    }
}

fn map_federation_invitation_insert_error(error: rusqlite::Error) -> TaskStoreError {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("UNIQUE constraint failed") =>
        {
            TaskStoreError::FederationInvitationConflict
        }
        other => TaskStoreError::Sql(other),
    }
}

fn map_federation_claim_insert_error(error: rusqlite::Error) -> TaskStoreError {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("UNIQUE constraint failed") =>
        {
            TaskStoreError::FederationClaimConflict
        }
        other => TaskStoreError::Sql(other),
    }
}

fn map_join_invitation_insert_error(error: rusqlite::Error) -> TaskStoreError {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("UNIQUE constraint failed") =>
        {
            TaskStoreError::FederationInvitationConflict
        }
        other => TaskStoreError::Sql(other),
    }
}

fn federation_join_invitation_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationJoinInvitation> {
    Ok(FederationJoinInvitation {
        invitation_id: parse_domain_id(&row.get::<_, String>(0)?)?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        apiary_name: row.get(2)?,
        shared_work_backend: row
            .get::<_, String>(3)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        required_policy_revision: row.get(4)?,
        promoted_project_catalog_digest: row.get(5)?,
        promoted_projects: Vec::new(),
        keeper_node_id: parse_domain_id(&row.get::<_, String>(6)?)?,
        keeper_hive_id: parse_domain_id(&row.get::<_, String>(7)?)?,
        keeper_hive_name: row.get(8)?,
        keeper_operator_id: parse_domain_id(&row.get::<_, String>(9)?)?,
        keeper_operator_display_name: row.get(10)?,
        keeper_endpoint: row.get(11)?,
        state: row
            .get::<_, String>(12)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        imported_at: row.get(13)?,
        expires_at: row.get(14)?,
    })
}

fn load_apiary_join_links(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
    now: i64,
) -> Result<Vec<ApiaryJoinLink>, TaskStoreError> {
    let mut statement = connection.prepare(
        "SELECT l.id, a.name, l.keeper_endpoint, l.state,
                l.candidate_hive_id, l.created_at, l.expires_at
         FROM apiary_join_links l
         JOIN apiaries a ON a.id = l.apiary_id
         WHERE l.apiary_id = ?1
         ORDER BY l.created_at DESC, l.id DESC",
    )?;
    let rows = statement
        .query_map([apiary_id.to_string()], |row| {
            Ok((
                parse_domain_id::<ApiaryJoinLinkId>(&row.get::<_, String>(0)?)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    rows.into_iter()
        .map(
            |(
                id,
                apiary_name,
                keeper_endpoint,
                stored_state,
                candidate_hive_id,
                issued_at,
                expires_at,
            )| {
                let state = if expires_at <= now
                    && !matches!(stored_state.as_str(), "invitation_issued" | "revoked")
                {
                    ApiaryJoinLinkState::Expired
                } else {
                    stored_state
                        .parse()
                        .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?
                };
                let candidate = candidate_hive_id
                    .map(|hive_id| {
                        let hive_id = parse_domain_id(&hive_id)
                            .map_err(|_| TaskStoreError::InvalidApiaryJoinLink)?;
                        candidate_by_hive(connection, apiary_id, hive_id)
                            .map_err(TaskStoreError::from)?
                            .ok_or(TaskStoreError::InvalidApiaryJoinLink)
                    })
                    .transpose()?;
                Ok(ApiaryJoinLink {
                    id,
                    apiary_id,
                    apiary_name,
                    keeper_endpoint,
                    state,
                    candidate,
                    issued_at,
                    expires_at,
                })
            },
        )
        .collect()
}

fn load_local_apiary_keeper_link(
    connection: &rusqlite::Connection,
    link_id: ApiaryJoinLinkId,
) -> rusqlite::Result<Option<ApiaryKeeperLink>> {
    connection
        .query_row(
            "SELECT link_id, keeper_endpoint, apiary_name, state,
                    created_at, updated_at, expires_at
             FROM local_apiary_keeper_links WHERE link_id = ?1",
            [link_id.to_string()],
            local_apiary_keeper_link_from_row,
        )
        .optional()
}

fn local_apiary_keeper_link_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ApiaryKeeperLink> {
    Ok(ApiaryKeeperLink {
        link_id: parse_domain_id(&row.get::<_, String>(0)?)?,
        keeper_endpoint: row.get(1)?,
        apiary_name: row.get(2)?,
        state: row
            .get::<_, String>(3)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        expires_at: row.get(6)?,
    })
}

fn candidate_by_hive(
    connection: &rusqlite::Connection,
    apiary_id: ApiaryId,
    hive_id: HiveId,
) -> rusqlite::Result<Option<ApiaryHiveCandidate>> {
    connection
        .query_row(
            "SELECT apiary_id, node_id, hive_id, hive_name, operator_id,
                    operator_display_name, public_key, card_issued_at,
                    card_expires_at, pinned_by_operator_id, pinned_at,
                    last_verified_at
             FROM apiary_hive_candidates WHERE apiary_id = ?1 AND hive_id = ?2",
            params![apiary_id.to_string(), hive_id.to_string()],
            candidate_from_row,
        )
        .optional()
}

fn candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiaryHiveCandidate> {
    Ok(ApiaryHiveCandidate {
        apiary_id: parse_domain_id(&row.get::<_, String>(0)?)?,
        node_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        hive_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        hive_name: row.get(3)?,
        operator_id: parse_domain_id(&row.get::<_, String>(4)?)?,
        operator_display_name: row.get(5)?,
        public_key: row.get(6)?,
        card_issued_at: row.get(7)?,
        card_expires_at: row.get(8)?,
        pinned_by_operator_id: parse_domain_id(&row.get::<_, String>(9)?)?,
        pinned_at: row.get(10)?,
        last_verified_at: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{SharedWorkBackend, StewardCapability};

    #[test]
    fn connection_card_is_stable_signed_bounded_and_private() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.issue_hive_connection_card(1_000, 3_600).unwrap();
        let second = store.issue_hive_connection_card(1_100, 3_600).unwrap();

        assert_eq!(first.payload.node_id, second.payload.node_id);
        assert_eq!(first.payload.public_key, second.payload.public_key);
        verify_hive_connection_card(&first, 1_001).unwrap();
        let serialized = serde_json::to_string(&first).unwrap();
        for forbidden in ["workspace", "terminal", "jira", "credential", "task"] {
            assert!(!serialized.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn connection_card_rejects_tampering_expiry_and_invalid_bounds() {
        let store = TaskStore::in_memory().unwrap();
        let card = store.issue_hive_connection_card(10_000, 3_600).unwrap();
        let mut tampered = card.clone();
        tampered.payload.hive_name = "Impostor Hive".into();
        assert!(verify_hive_connection_card(&tampered, 10_001).is_err());
        assert!(verify_hive_connection_card(&card, card.payload.expires_at).is_err());
        assert!(
            store
                .issue_hive_connection_card(10_000, MAX_CONNECTION_CARD_LIFETIME_SECONDS + 1)
                .is_err()
        );
    }

    #[test]
    fn keeper_join_link_is_durable_bounded_and_never_stores_its_secret() {
        let personal = TaskStore::in_memory().unwrap();
        assert!(matches!(
            personal.issue_apiary_join_link("https://keeper.example.test", 10_000, 3_600),
            Err(TaskStoreError::ApiaryKeeperRequired)
        ));

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let keeper = TaskStore::open(&path).unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let bundle = keeper
            .issue_apiary_join_link("https://keeper.example.test/swarm/", 10_000, 3_600)
            .unwrap();
        assert_eq!(
            bundle.link.keeper_endpoint,
            "https://keeper.example.test/swarm"
        );
        assert_eq!(bundle.link.state, ApiaryJoinLinkState::Open);
        assert_eq!(
            Base64UrlUnpadded::decode_vec(&bundle.one_time_secret)
                .unwrap()
                .len(),
            32
        );
        let stored: (usize, String) = keeper
            .connection()
            .unwrap()
            .query_row(
                "SELECT length(secret_digest), hex(secret_digest)
                 FROM apiary_join_links WHERE id = ?1",
                [bundle.link.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored.0, 32);
        assert!(!stored.1.contains(&bundle.one_time_secret));
        drop(keeper);

        let reopened = TaskStore::open(path).unwrap();
        assert_eq!(
            reopened.apiary_join_links(10_001).unwrap(),
            vec![bundle.link.clone()]
        );
        assert_eq!(
            reopened.apiary_join_links(bundle.link.expires_at).unwrap()[0].state,
            ApiaryJoinLinkState::Expired
        );
    }

    #[test]
    fn join_link_binds_one_signed_hive_and_requires_explicit_keeper_approval() {
        let first_hive = TaskStore::in_memory().unwrap();
        let first_card = first_hive
            .issue_hive_connection_card(10_000, 3_600)
            .unwrap();
        let second_hive = TaskStore::in_memory().unwrap();
        let second_card = second_hive
            .issue_hive_connection_card(10_000, 3_600)
            .unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let bundle = keeper
            .issue_apiary_join_link("https://keeper.example.test", 10_000, 3_600)
            .unwrap();

        assert!(matches!(
            keeper.present_apiary_join_link_identity(
                bundle.link.id,
                "wrong-secret",
                &first_card,
                10_001,
            ),
            Err(TaskStoreError::InvalidApiaryJoinLink)
        ));
        let waiting = keeper
            .present_apiary_join_link_identity(
                bundle.link.id,
                &bundle.one_time_secret,
                &first_card,
                10_001,
            )
            .unwrap();
        assert_eq!(waiting.state, ApiaryJoinLinkState::AwaitingApproval);
        assert_eq!(
            waiting
                .candidate
                .as_ref()
                .map(|candidate| candidate.hive_id),
            Some(first_card.payload.hive_id)
        );
        assert!(matches!(
            keeper.present_apiary_join_link_identity(
                bundle.link.id,
                &bundle.one_time_secret,
                &second_card,
                10_002,
            ),
            Err(TaskStoreError::InvalidApiaryJoinLink)
        ));

        let approved = keeper
            .approve_apiary_join_link(bundle.link.id, 10_003)
            .unwrap();
        assert_eq!(approved.state, ApiaryJoinLinkState::Approved);
        assert!(matches!(
            keeper.approve_apiary_join_link(bundle.link.id, 10_004),
            Err(TaskStoreError::ApiaryJoinLinkResolved)
        ));

        let issued = keeper
            .poll_apiary_join_link(bundle.link.id, &bundle.one_time_secret, 10_004)
            .unwrap();
        assert_eq!(issued.link.state, ApiaryJoinLinkState::InvitationIssued);
        let invitation = issued.invitation.unwrap();
        assert_eq!(invitation.one_time_secret, bundle.one_time_secret);
        assert_eq!(
            invitation.invitation.payload.invited_hive_id,
            first_card.payload.hive_id
        );
        let retry = keeper
            .poll_apiary_join_link(bundle.link.id, &bundle.one_time_secret, 10_005)
            .unwrap()
            .invitation
            .unwrap();
        assert_eq!(retry, invitation);
        let stored: String = keeper
            .connection()
            .unwrap()
            .query_row(
                "SELECT invitation_bundle_json FROM apiary_join_links WHERE id = ?1",
                [bundle.link.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stored.contains(&bundle.one_time_secret));
    }

    #[test]
    fn keeper_join_links_have_a_small_active_capability_budget() {
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        for offset in 0..MAX_ACTIVE_APIARY_JOIN_LINKS {
            keeper
                .issue_apiary_join_link(
                    "https://keeper.example.test",
                    10_000 + i64::try_from(offset).unwrap(),
                    3_600,
                )
                .unwrap();
        }
        assert!(matches!(
            keeper.issue_apiary_join_link("https://keeper.example.test", 10_100, 3_600),
            Err(TaskStoreError::ApiaryJoinLinkLimit)
        ));
    }

    #[test]
    fn personal_hive_retains_keeper_polling_secret_privately_across_restart() {
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let bundle = keeper
            .issue_apiary_join_link("https://keeper.example.test", 10_000, 3_600)
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("member.sqlite3");
        let member = TaskStore::open(&path).unwrap();
        let saved = member
            .save_local_apiary_keeper_link(
                bundle.link.id,
                &bundle.link.keeper_endpoint,
                &bundle.one_time_secret,
                10_001,
            )
            .unwrap();
        assert_eq!(saved.state, ApiaryJoinLinkState::Open);
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains(&bundle.one_time_secret)
        );
        assert!(matches!(
            member.save_local_apiary_keeper_link(
                bundle.link.id,
                &bundle.link.keeper_endpoint,
                &bundle.one_time_secret,
                10_002,
            ),
            Err(TaskStoreError::ApiaryJoinLinkResolved)
        ));
        drop(member);

        let reopened = TaskStore::open(path).unwrap();
        assert_eq!(reopened.local_apiary_keeper_links().unwrap(), vec![saved]);
        assert_eq!(
            reopened
                .local_apiary_keeper_link_credential(bundle.link.id)
                .unwrap(),
            (
                bundle.link.keeper_endpoint.clone(),
                bundle.one_time_secret.clone()
            )
        );
        let updated = reopened
            .update_local_apiary_keeper_link(&bundle.link, 10_003)
            .unwrap();
        assert_eq!(updated.apiary_name.as_deref(), Some("Garden"));
        assert_eq!(updated.expires_at, Some(bundle.link.expires_at));
        reopened
            .remove_local_apiary_keeper_link(bundle.link.id)
            .unwrap();
        assert!(reopened.local_apiary_keeper_links().unwrap().is_empty());
    }

    #[test]
    fn keeper_pins_a_verified_card_from_an_independent_hive_without_membership() {
        let remote = TaskStore::in_memory().unwrap();
        let remote_identity = remote.local_hive_identity().unwrap();
        let card = remote.issue_hive_connection_card(10_000, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };

        let candidate = keeper.pin_hive_candidate(&card, 10_001).unwrap();
        assert_eq!(candidate.apiary_id, apiary.id);
        assert_eq!(candidate.hive_id, remote_identity.hive.id);
        assert_eq!(candidate.operator_id, remote_identity.operator.id);
        assert_eq!(keeper.list_hive_candidates().unwrap(), vec![candidate]);
        assert_eq!(
            keeper
                .connection()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM hives", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            keeper.local_hive_identity().unwrap().hive.apiary_id,
            Some(apiary.id)
        );
    }

    #[test]
    fn candidate_pin_rejects_non_keepers_self_identity_and_key_replacement() {
        let remote = TaskStore::in_memory().unwrap();
        let card = remote.issue_hive_connection_card(10_000, 3_600).unwrap();
        let personal = TaskStore::in_memory().unwrap();
        assert!(matches!(
            personal.pin_hive_candidate(&card, 10_001),
            Err(TaskStoreError::ApiaryKeeperRequired)
        ));

        let keeper = TaskStore::in_memory().unwrap();
        let self_card = keeper.issue_hive_connection_card(10_000, 3_600).unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        assert!(matches!(
            keeper.pin_hive_candidate(&self_card, 10_001),
            Err(TaskStoreError::InvalidFederationConnectionCard)
        ));

        let identity = keeper.local_hive_identity().unwrap();
        let apiary_id = identity.hive.apiary_id.unwrap();
        keeper
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO apiary_hive_candidates
                    (apiary_id, node_id, hive_id, hive_name, operator_id,
                     operator_display_name, public_key, card_issued_at,
                     card_expires_at, pinned_by_operator_id, pinned_at,
                     last_verified_at)
                 VALUES (?1, ?2, ?3, 'Pinned', ?4, 'Remote', 'different',
                         9000, 12000, ?5, 9001, 9001)",
                params![
                    apiary_id.to_string(),
                    FederationNodeId::new().to_string(),
                    card.payload.hive_id.to_string(),
                    card.payload.operator_id.to_string(),
                    identity.operator.id.to_string(),
                ],
            )
            .unwrap();
        assert!(matches!(
            keeper.pin_hive_candidate(&card, 10_001),
            Err(TaskStoreError::HiveCandidateIdentityConflict)
        ));
    }

    #[test]
    fn corrupt_durable_key_material_fails_closed() {
        let store = TaskStore::in_memory().unwrap();
        store.issue_hive_connection_card(10_000, 3_600).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE local_federation_identity SET public_key = zeroblob(32)",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.issue_hive_connection_card(10_001, 3_600),
            Err(TaskStoreError::InvalidFederationIdentity)
        ));
    }

    #[test]
    fn federation_identity_survives_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let first = TaskStore::open(&path)
            .unwrap()
            .issue_hive_connection_card(10_000, 3_600)
            .unwrap();
        let second = TaskStore::open(path)
            .unwrap()
            .issue_hive_connection_card(10_100, 3_600)
            .unwrap();

        assert_eq!(first.payload.node_id, second.payload.node_id);
        assert_eq!(first.payload.public_key, second.payload.public_key);
        assert_ne!(first.signature, second.signature);
    }

    #[test]
    fn keeper_issues_one_signed_secret_once_for_an_exact_pinned_hive() {
        let remote = TaskStore::in_memory().unwrap();
        let remote_card = remote.issue_hive_connection_card(10_000, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&remote_card, 10_001).unwrap();

        let bundle = keeper
            .issue_apiary_invitation_bundle(
                candidate.hive_id,
                "https://keeper.example.test/swarm",
                10_100,
                3_600,
            )
            .unwrap();
        verify_apiary_invitation_envelope(
            &bundle.invitation,
            &bundle.keeper_connection_card.payload.public_key,
            10_101,
        )
        .unwrap();
        assert_eq!(bundle.invitation.payload.invited_hive_id, candidate.hive_id);
        assert_eq!(bundle.invitation.payload.invited_node_id, candidate.node_id);
        assert_eq!(
            bundle.invitation.payload.invited_operator_id,
            candidate.operator_id
        );
        assert_eq!(
            bundle.invitation.payload.shared_work_backend,
            SharedWorkBackend::Jira
        );
        assert_eq!(bundle.invitation.payload.required_policy_revision, 1);
        assert_eq!(
            Base64UrlUnpadded::decode_vec(&bundle.one_time_secret)
                .unwrap()
                .len(),
            32
        );
        let stored: (usize, usize, String) = keeper
            .connection()
            .unwrap()
            .query_row(
                "SELECT length(secret_digest), length(nonce), envelope_json
                 FROM apiary_federation_invitations WHERE id = ?1",
                [bundle.invitation.payload.invitation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(stored.0, 32);
        assert!(!stored.2.contains(&bundle.one_time_secret));
        assert_eq!(
            keeper
                .pending_federation_invitation_count(candidate.hive_id, 10_101)
                .unwrap(),
            1
        );
        assert!(matches!(
            keeper.issue_apiary_invitation_bundle(
                candidate.hive_id,
                "https://keeper.example.test/swarm",
                10_102,
                3_600,
            ),
            Err(TaskStoreError::FederationInvitationConflict)
        ));
        assert_eq!(
            keeper
                .apiary_collapse_readiness(candidate.apiary_id)
                .unwrap()
                .pending_invitation_count,
            1
        );
    }

    #[test]
    fn invitation_envelope_rejects_wrong_keys_tampering_expiry_and_insecure_endpoints() {
        let remote = TaskStore::in_memory().unwrap();
        let remote_card = remote.issue_hive_connection_card(10_000, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&remote_card, 10_001).unwrap();
        assert!(matches!(
            keeper.issue_apiary_invitation_bundle(
                candidate.hive_id,
                "http://keeper.example.test",
                10_100,
                3_600,
            ),
            Err(TaskStoreError::InvalidFederationInvitation)
        ));
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                candidate.hive_id,
                "http://127.0.0.1:8766",
                10_100,
                3_600,
            )
            .unwrap();
        assert!(
            verify_apiary_invitation_envelope(
                &bundle.invitation,
                &remote_card.payload.public_key,
                10_101,
            )
            .is_err()
        );
        let mut tampered = bundle.invitation.clone();
        tampered.payload.apiary_name = "Impostor Garden".into();
        assert!(
            verify_apiary_invitation_envelope(
                &tampered,
                &bundle.keeper_connection_card.payload.public_key,
                10_101,
            )
            .is_err()
        );
        assert!(
            verify_apiary_invitation_envelope(
                &bundle.invitation,
                &bundle.keeper_connection_card.payload.public_key,
                bundle.invitation.payload.expires_at,
            )
            .is_err()
        );
    }

    #[test]
    fn exact_invited_hive_pins_keeper_without_joining_or_exposing_the_secret() {
        let invited = TaskStore::in_memory().unwrap();
        let invited_identity = invited.local_hive_identity().unwrap();
        let invited_card = invited.issue_hive_connection_card(10_000, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };
        let keeper_operator_id = keeper.local_hive_identity().unwrap().operator.id;
        keeper
            .promote_apiary_jira_project(
                apiary.id,
                "10000",
                "WWD",
                "Website Development",
                keeper_operator_id,
                10_000,
            )
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&invited_card, 10_001).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                candidate.hive_id,
                "https://keeper.example.test/swarm",
                10_100,
                3_600,
            )
            .unwrap();

        let imported = invited
            .import_apiary_invitation_bundle(&bundle, 10_101)
            .unwrap();
        assert_eq!(imported.apiary_name, "Garden");
        assert_eq!(imported.state, FederationJoinInvitationState::KeeperPinned);
        assert_eq!(imported.keeper_hive_name, "My Hive");
        assert_eq!(
            imported.promoted_projects,
            vec![FederationProjectManifestEntry {
                project_id: "10000".into(),
                project_key: "WWD".into(),
                project_name: "Website Development".into(),
            }]
        );
        assert_eq!(invited.local_hive_identity().unwrap().hive.apiary_id, None);
        assert_eq!(
            invited.federation_join_invitations(10_102).unwrap(),
            vec![imported.clone()]
        );
        let stored: (usize, String) = invited
            .connection()
            .unwrap()
            .query_row(
                "SELECT length(one_time_secret), envelope_json
                 FROM apiary_join_invitations WHERE id = ?1",
                [bundle.invitation.payload.invitation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored.0, 32);
        assert!(!stored.1.contains(&bundle.one_time_secret));
        assert_eq!(
            invited
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM apiary_join_invitation_projects
                     WHERE invitation_id = ?1 AND project_key = 'WWD'",
                    [bundle.invitation.payload.invitation_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(invited_identity.hive.id, candidate.hive_id);
        assert_imported_policy_and_project_readiness(&invited, imported.invitation_id);
        assert!(matches!(
            invited.import_apiary_invitation_bundle(&bundle, 10_102),
            Err(TaskStoreError::FederationInvitationConflict)
        ));
    }

    fn assert_imported_policy_and_project_readiness(
        invited: &TaskStore,
        invitation_id: ApiaryInvitationId,
    ) {
        let project_readiness = invited.federation_project_readiness(invitation_id).unwrap();
        assert_eq!(project_readiness.len(), 1);
        assert!(!project_readiness[0].is_ready());
        assert!(matches!(
            invited.accept_federation_join_policy(invitation_id, 2, 10_102),
            Err(TaskStoreError::ApiaryJoinNotReady)
        ));
        let accepted = invited
            .accept_federation_join_policy(invitation_id, 1, 10_102)
            .unwrap();
        assert_eq!(
            accepted.state,
            FederationJoinInvitationState::PolicyAccepted
        );
        let binding = invited
            .upsert_jira_project_binding(&crate::JiraProjectBindingInput {
                project_id: "10000",
                project_key: "RENAMED",
                project_name: "Renamed locally",
                scope: swarm_domain::JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        invited
            .replace_jira_status_mappings(
                binding.id,
                &[swarm_domain::JiraStatusMapping {
                    jira_status_id: "1".into(),
                    jira_status_name: "To Do".into(),
                    task_state: swarm_domain::TaskState::Ready,
                }],
            )
            .unwrap();
        let project_readiness = invited.federation_project_readiness(invitation_id).unwrap();
        assert_eq!(project_readiness[0].binding_id, Some(binding.id));
        assert!(project_readiness[0].is_ready());
    }

    #[test]
    fn invitation_import_rejects_the_wrong_hive_tampering_and_existing_membership() {
        let invited = TaskStore::in_memory().unwrap();
        let invited_card = invited.issue_hive_connection_card(10_000, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };
        keeper
            .promote_apiary_jira_project(
                apiary.id,
                "10000",
                "WWD",
                "Website Development",
                keeper.local_hive_identity().unwrap().operator.id,
                10_000,
            )
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&invited_card, 10_001).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                candidate.hive_id,
                "https://keeper.example.test/swarm",
                10_100,
                3_600,
            )
            .unwrap();

        let wrong_hive = TaskStore::in_memory().unwrap();
        assert!(matches!(
            wrong_hive.import_apiary_invitation_bundle(&bundle, 10_101),
            Err(TaskStoreError::InvalidFederationInvitation)
        ));
        let mut tampered = bundle.clone();
        tampered.one_time_secret = Base64UrlUnpadded::encode_string(&[7_u8; 31]);
        assert!(matches!(
            invited.import_apiary_invitation_bundle(&tampered, 10_101),
            Err(TaskStoreError::InvalidFederationInvitation)
        ));
        let mut tampered_manifest = bundle.clone();
        tampered_manifest.promoted_projects[0].project_name = "Tampered".into();
        assert!(matches!(
            invited.import_apiary_invitation_bundle(&tampered_manifest, 10_101),
            Err(TaskStoreError::InvalidFederationInvitation)
        ));
        invited
            .create_apiary_for_local_hive("Other", SharedWorkBackend::Jira, 10_050)
            .unwrap();
        assert!(matches!(
            invited.import_apiary_invitation_bundle(&bundle, 10_101),
            Err(TaskStoreError::ApiaryMembershipConflict)
        ));
    }

    #[test]
    fn schema_v30_migrates_to_separate_hive_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_hive_candidates;
                 PRAGMA user_version = 30;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_hive_candidates'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v31_migrates_to_one_time_federation_invitations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_federation_invitations;
                 PRAGMA user_version = 31;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_federation_invitations'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v32_migrates_to_private_join_invitations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_join_invitations;
                 PRAGMA user_version = 32;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_join_invitations'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v33_migrates_to_private_invitation_project_manifests() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_join_invitation_projects;
                 PRAGMA user_version = 33;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_join_invitation_projects'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn independent_hive_submission_is_consumed_once_with_a_signed_retry_stable_receipt() {
        let now = 20_000;
        let invited = TaskStore::in_memory().unwrap();
        let invited_identity = invited.local_hive_identity().unwrap();
        let invited_card = invited.issue_hive_connection_card(now, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, now - 1)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };
        keeper.pin_hive_candidate(&invited_card, now).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                invited_identity.hive.id,
                "https://keeper.example.test/swarm",
                now,
                3_600,
            )
            .unwrap();
        let invitation = invited
            .import_apiary_invitation_bundle(&bundle, now + 1)
            .unwrap();
        invited
            .accept_federation_join_policy(invitation.invitation_id, 1, now + 2)
            .unwrap();
        let readiness = FederationJoinReadiness {
            jira_connection: swarm_domain::JiraConnectionState::Ready,
            projects: Vec::new(),
            blockers: Vec::new(),
        };
        let submission = invited
            .prepare_federation_join_submission(invitation.invitation_id, &readiness, now + 3)
            .unwrap();
        assert_eq!(
            submission,
            invited
                .prepare_federation_join_submission(invitation.invitation_id, &readiness, now + 4,)
                .unwrap()
        );

        let accepted = keeper
            .consume_federation_join_submission(&submission, now + 4)
            .unwrap();
        verify_federation_membership_receipt(
            &accepted.receipt,
            &bundle.keeper_connection_card.payload.public_key,
            now + 4,
        )
        .unwrap();
        assert_eq!(accepted.receipt.payload.apiary_id, apiary.id);
        assert_eq!(
            accepted.receipt.payload.member_hive_id,
            invited_identity.hive.id
        );
        assert_eq!(
            accepted,
            keeper
                .consume_federation_join_submission(&submission, now + 5)
                .unwrap()
        );
        assert_signed_catalog(
            &keeper,
            &accepted,
            &bundle.keeper_connection_card.payload.public_key,
            apiary.id,
            now + 5,
        );
        assert_eq!(
            keeper
                .connection()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM hives", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            keeper
                .pending_federation_invitation_count(invited_identity.hive.id, now + 5)
                .unwrap(),
            0
        );
        let joined = invited
            .apply_federation_join_acceptance(invitation.invitation_id, &accepted, now + 5)
            .unwrap();
        assert!(matches!(
            joined,
            swarm_domain::LocalApiaryContext::Federated {
                local_role: swarm_domain::LocalApiaryRole::Member,
                ..
            }
        ));
        assert_eq!(
            invited
                .apply_federation_join_acceptance(invitation.invitation_id, &accepted, now + 6,)
                .unwrap(),
            joined
        );
        assert_member_acknowledges_catalog(&invited, &keeper, &accepted, apiary.id, now + 7);

        assert_tampered_submission_is_rejected(&keeper, submission, now + 6);
    }

    #[test]
    fn keeper_serializes_member_claims_and_preserves_confirmed_home_hive() {
        let now = 30_000;
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, now - 10)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };
        let first_member = TaskStore::in_memory().unwrap();
        let second_member = TaskStore::in_memory().unwrap();
        let first = register_remote_member(&keeper, &first_member, now);
        let second = register_remote_member(&keeper, &second_member, now + 10);
        let keeper_operator_id = keeper.local_hive_identity().unwrap().operator.id;
        keeper
            .promote_apiary_jira_project(
                apiary.id,
                "10001",
                "WWD",
                "Website Development",
                keeper_operator_id,
                now + 20,
            )
            .unwrap();

        let reserved = keeper
            .reserve_federation_claim(
                &first.node_credential,
                "10001",
                "20001",
                "WWD-101",
                now + 21,
            )
            .unwrap();
        assert_eq!(reserved.state, FederationClaimState::Reserved);
        assert_eq!(reserved.home_hive_id, first.receipt.payload.member_hive_id);
        assert_eq!(
            keeper
                .reserve_federation_claim(
                    &first.node_credential,
                    "10001",
                    "20001",
                    "WWD-101",
                    now + 22,
                )
                .unwrap(),
            reserved
        );
        assert!(matches!(
            keeper.reserve_federation_claim(
                &second.node_credential,
                "10001",
                "20001",
                "WWD-101",
                now + 22,
            ),
            Err(TaskStoreError::FederationClaimConflict)
        ));

        let confirmed = keeper
            .confirm_federation_claim(&first.node_credential, reserved.id, now + 23)
            .unwrap();
        assert_eq!(confirmed.state, FederationClaimState::Confirmed);
        assert_eq!(confirmed.confirmed_at, Some(now + 23));
        assert_eq!(
            keeper
                .confirm_federation_claim(&first.node_credential, reserved.id, now + 24)
                .unwrap(),
            confirmed
        );
        assert!(matches!(
            keeper.release_federation_claim(&first.node_credential, reserved.id, now + 24),
            Err(TaskStoreError::InvalidFederationClaim)
        ));
        assert!(matches!(
            keeper.reserve_federation_claim(
                &second.node_credential,
                "10001",
                "20001",
                "WWD-101",
                now + 24,
            ),
            Err(TaskStoreError::FederationClaimConflict)
        ));
        assert_eq!(
            keeper.list_active_federation_claims(now + 24).unwrap(),
            vec![confirmed]
        );
        assert!(matches!(
            first_member.list_active_federation_claims(now + 24),
            Err(TaskStoreError::ApiaryKeeperRequired)
        ));
    }

    #[test]
    fn unconfirmed_claims_can_be_released_or_recovered_after_expiry() {
        let now = 40_000;
        let keeper = TaskStore::in_memory().unwrap();
        let context = keeper
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, now - 10)
            .unwrap();
        let swarm_domain::LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected Keeper Apiary");
        };
        let first_member = TaskStore::in_memory().unwrap();
        let second_member = TaskStore::in_memory().unwrap();
        let first = register_remote_member(&keeper, &first_member, now);
        let second = register_remote_member(&keeper, &second_member, now + 10);
        keeper
            .promote_apiary_jira_project(
                apiary.id,
                "10001",
                "WWD",
                "Website Development",
                keeper.local_hive_identity().unwrap().operator.id,
                now + 20,
            )
            .unwrap();

        let releasable = keeper
            .reserve_federation_claim(
                &first.node_credential,
                "10001",
                "20002",
                "WWD-102",
                now + 21,
            )
            .unwrap();
        let released = keeper
            .release_federation_claim(&first.node_credential, releasable.id, now + 22)
            .unwrap();
        assert_eq!(released.state, FederationClaimState::Released);
        let reclaimed = keeper
            .reserve_federation_claim(
                &second.node_credential,
                "10001",
                "20002",
                "WWD-102",
                now + 23,
            )
            .unwrap();
        assert_eq!(
            reclaimed.home_hive_id,
            second.receipt.payload.member_hive_id
        );

        let expiring = keeper
            .reserve_federation_claim(
                &first.node_credential,
                "10001",
                "20003",
                "WWD-103",
                now + 24,
            )
            .unwrap();
        let after_expiry = expiring.reservation_expires_at;
        let recovered = keeper
            .reserve_federation_claim(
                &second.node_credential,
                "10001",
                "20003",
                "WWD-103",
                after_expiry,
            )
            .unwrap();
        assert_eq!(
            recovered.home_hive_id,
            second.receipt.payload.member_hive_id
        );
        let prior_state = keeper
            .connection()
            .unwrap()
            .query_row(
                "SELECT state FROM apiary_federation_claims WHERE id = ?1",
                [expiring.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(prior_state, "expired");
        let visible = keeper.list_active_federation_claims(after_expiry).unwrap();
        assert_eq!(
            visible.iter().map(|claim| claim.id).collect::<Vec<_>>(),
            vec![recovered.id]
        );
    }

    fn register_remote_member(
        keeper: &TaskStore,
        member: &TaskStore,
        now: i64,
    ) -> FederationJoinAcceptance {
        let identity = member.local_hive_identity().unwrap();
        let card = member.issue_hive_connection_card(now, 3_600).unwrap();
        keeper.pin_hive_candidate(&card, now).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                identity.hive.id,
                "https://keeper.example.test/swarm",
                now,
                3_600,
            )
            .unwrap();
        let invitation = member
            .import_apiary_invitation_bundle(&bundle, now + 1)
            .unwrap();
        member
            .accept_federation_join_policy(invitation.invitation_id, 1, now + 2)
            .unwrap();
        let readiness = FederationJoinReadiness {
            jira_connection: swarm_domain::JiraConnectionState::Ready,
            projects: Vec::new(),
            blockers: Vec::new(),
        };
        let submission = member
            .prepare_federation_join_submission(invitation.invitation_id, &readiness, now + 3)
            .unwrap();
        keeper
            .consume_federation_join_submission(&submission, now + 4)
            .unwrap()
    }

    fn joined_member(now: i64) -> (TaskStore, TaskStore) {
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .unwrap();
        let member = TaskStore::in_memory().unwrap();
        let acceptance = register_remote_member(&keeper, &member, now + 1);
        member
            .apply_federation_join_acceptance(
                acceptance.receipt.payload.invitation_id,
                &acceptance,
                now + 6,
            )
            .unwrap();
        (keeper, member)
    }

    #[test]
    fn member_stewardship_projection_syncs_and_revokes_exact_authority() {
        let now = 79_000;
        let (keeper, member) = joined_member(now);
        let identity = member.local_hive_identity().unwrap();
        let connection = member.federation_member_connection().unwrap();
        let granted = keeper
            .set_stewardship(
                identity.operator.id,
                &[identity.hive.id],
                &[
                    StewardCapability::Observe,
                    StewardCapability::Assist,
                    StewardCapability::Takeover,
                ],
                now + 10,
            )
            .unwrap();

        let snapshot = keeper
            .federation_stewardship_snapshot(&connection.node_credential, now + 11)
            .unwrap();
        assert_eq!(snapshot.member_operator_id, identity.operator.id);
        assert_eq!(snapshot.stewardship.as_ref(), Some(&granted));
        member
            .apply_federation_stewardship_snapshot(&snapshot, now + 12)
            .unwrap();
        member
            .apply_federation_stewardship_snapshot(&snapshot, now + 13)
            .unwrap();
        assert_eq!(
            member.local_federation_stewardship_snapshot().unwrap(),
            Some(snapshot.clone())
        );

        let mut foreign = snapshot.clone();
        foreign.member_operator_id = keeper.local_hive_identity().unwrap().operator.id;
        assert!(matches!(
            member.apply_federation_stewardship_snapshot(&foreign, now + 14),
            Err(TaskStoreError::InvalidStewardship)
        ));

        keeper.revoke_stewardship(granted.id, now + 15).unwrap();
        let revoked = keeper
            .federation_stewardship_snapshot(&connection.node_credential, now + 16)
            .unwrap();
        assert_eq!(revoked.stewardship, None);
        member
            .apply_federation_stewardship_snapshot(&revoked, now + 17)
            .unwrap();
        assert_eq!(
            member
                .local_federation_stewardship_snapshot()
                .unwrap()
                .unwrap()
                .stewardship,
            None
        );
    }

    #[test]
    fn member_sync_health_is_durable_bounded_and_content_free() {
        let now = 80_000;
        let (keeper, member) = joined_member(now);
        assert_eq!(
            member.federation_sync_health().unwrap(),
            FederationSyncHealth::default()
        );

        let first = member
            .record_federation_sync_failure(FederationSyncCondition::Offline, now + 10)
            .unwrap();
        assert_eq!(first.consecutive_failures, 1);
        assert_eq!(first.next_attempt_at, Some(now + 15));
        let second = member
            .record_federation_sync_failure(FederationSyncCondition::Offline, now + 20)
            .unwrap();
        assert_eq!(second.consecutive_failures, 2);
        assert_eq!(second.next_attempt_at, Some(now + 35));

        let current = member.record_federation_sync_success(now + 40).unwrap();
        assert_eq!(current.condition, FederationSyncCondition::Current);
        assert_eq!(current.consecutive_failures, 0);
        assert_eq!(current.last_success_at, Some(now + 40));
        assert_eq!(current.next_attempt_at, Some(now + 100));

        let halted = member
            .record_federation_sync_failure(
                FederationSyncCondition::AuthenticationRequired,
                now + 50,
            )
            .unwrap();
        assert_eq!(halted.last_success_at, Some(now + 40));
        assert_eq!(halted.next_attempt_at, None);
        assert_eq!(member.federation_sync_health().unwrap(), halted);

        assert!(matches!(
            member.record_federation_sync_failure(FederationSyncCondition::Current, now + 60),
            Err(TaskStoreError::InvalidFederationSync)
        ));
        assert!(matches!(
            keeper.federation_sync_health(),
            Err(TaskStoreError::InvalidFederationSync)
        ));
        assert!(matches!(
            TaskStore::in_memory().unwrap().federation_sync_health(),
            Err(TaskStoreError::InvalidFederationSync)
        ));

        let serialized = serde_json::to_string(&halted).unwrap().to_ascii_lowercase();
        for forbidden in ["endpoint", "credential", "jira", "task", "response"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    fn assert_tampered_submission_is_rejected(
        keeper: &TaskStore,
        mut submission: FederationJoinSubmission,
        now: i64,
    ) {
        submission.payload.required_policy_revision += 1;
        assert!(matches!(
            keeper.consume_federation_join_submission(&submission, now),
            Err(TaskStoreError::InvalidFederationInvitation)
        ));
    }

    fn assert_member_acknowledges_catalog(
        member: &TaskStore,
        keeper: &TaskStore,
        acceptance: &FederationJoinAcceptance,
        apiary_id: ApiaryId,
        now: i64,
    ) {
        let catalog = keeper
            .signed_federation_catalog(&acceptance.node_credential, now)
            .unwrap();
        let acknowledgement = member
            .acknowledge_federation_catalog(&catalog, now)
            .unwrap();
        assert_eq!(acknowledgement.apiary_id, apiary_id);
        assert_eq!(
            member.federation_catalog_acknowledgement().unwrap(),
            Some(acknowledgement.clone())
        );
        assert_eq!(
            member
                .acknowledge_federation_catalog(&catalog, now + 1)
                .unwrap(),
            acknowledgement
        );
        let mut altered = catalog;
        altered.payload.policy_revision += 1;
        assert!(matches!(
            member.acknowledge_federation_catalog(&altered, now + 1),
            Err(TaskStoreError::InvalidFederationCredential)
        ));
    }

    fn assert_signed_catalog(
        keeper: &TaskStore,
        accepted: &FederationJoinAcceptance,
        keeper_public_key: &str,
        apiary_id: ApiaryId,
        now: i64,
    ) {
        let catalog = keeper
            .signed_federation_catalog(&accepted.node_credential, now)
            .unwrap();
        verify_federation_catalog_snapshot(&catalog, keeper_public_key, &accepted.receipt, now)
            .unwrap();
        assert_eq!(catalog.payload.apiary_id, apiary_id);
        assert!(catalog.payload.projects.is_empty());
        assert!(matches!(
            keeper.signed_federation_catalog("not-a-credential", now),
            Err(TaskStoreError::InvalidFederationCredential)
        ));
        let mut tampered = catalog;
        tampered.payload.policy_revision += 1;
        assert!(matches!(
            verify_federation_catalog_snapshot(
                &tampered,
                keeper_public_key,
                &accepted.receipt,
                now,
            ),
            Err(TaskStoreError::InvalidFederationCredential)
        ));
    }

    #[test]
    fn schema_v34_migrates_to_federation_memberships() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_federation_memberships;
                 ALTER TABLE apiary_join_invitations DROP COLUMN submission_json;
                 PRAGMA user_version = 34;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_federation_memberships'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v35_migrates_to_local_federation_membership() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE local_federation_membership;
                 PRAGMA user_version = 35;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'local_federation_membership'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v36_migrates_to_local_federation_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE local_federation_catalog;
                 PRAGMA user_version = 36;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'local_federation_catalog'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v37_migrates_to_federation_claims() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE apiary_federation_claims;
                 PRAGMA user_version = 37;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'apiary_federation_claims'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn schema_v38_migrates_to_local_federation_sync() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE local_federation_sync;
                 PRAGMA user_version = 38;",
            )
            .unwrap();
        drop(store);

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'local_federation_sync'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }
}
