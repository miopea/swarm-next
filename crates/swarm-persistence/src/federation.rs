use std::{collections::HashSet, str::FromStr};

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use swarm_domain::{
    ApiaryHiveCandidate, ApiaryId, ApiaryInvitationBundle, ApiaryInvitationEnvelope,
    ApiaryInvitationEnvelopePayload, ApiaryInvitationId, FEDERATION_CONNECTION_CARD_SCHEMA_VERSION,
    FEDERATION_INVITATION_SCHEMA_VERSION, FEDERATION_PROTOCOL_VERSION, FederationJoinInvitation,
    FederationJoinInvitationState, FederationNodeId, FederationProjectManifestEntry,
    FederationProjectReadiness, HiveConnectionCard, HiveConnectionCardPayload, HiveId,
    JiraProjectBindingId, SharedWorkBackend,
};
use url::Url;

use crate::{TaskStore, TaskStoreError, parse_domain_id};

pub const MIN_CONNECTION_CARD_LIFETIME_SECONDS: i64 = 5 * 60;
pub const MAX_CONNECTION_CARD_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const MIN_FEDERATION_INVITATION_LIFETIME_SECONDS: i64 = 5 * 60;
pub const MAX_FEDERATION_INVITATION_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_CLOCK_SKEW_SECONDS: i64 = 5 * 60;
const MAX_KEEPER_ENDPOINT_BYTES: usize = 2_048;
const MAX_PROMOTED_PROJECTS_PER_INVITATION: usize = 1_000;

struct LocalFederationIdentity {
    node_id: FederationNodeId,
    signing_key: SigningKey,
}

struct KeeperInvitationContext {
    apiary_name: String,
    backend: SharedWorkBackend,
    policy_revision: u64,
}

impl TaskStore {
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
        let (secret, one_time_secret, nonce) = invitation_material()?;
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
    let mut secret = [0_u8; 32];
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut secret).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
    getrandom::fill(&mut nonce).map_err(|_| TaskStoreError::FederationEntropyUnavailable)?;
    let one_time_secret = Base64UrlUnpadded::encode_string(&secret);
    let nonce = Base64UrlUnpadded::encode_string(&nonce);
    Ok((secret, one_time_secret, nonce))
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
    use swarm_domain::SharedWorkBackend;

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
}
