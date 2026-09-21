//! Apiary policy with a body: Keeper authors shared defaults, members apply
//! them, and the gap is computable from both sides.

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use swarm_domain::{
    APIARY_POLICY_SCHEMA_VERSION, ApiaryPolicySetting, ApiaryPolicySnapshot,
    ApiaryPolicySnapshotPayload, FEDERATION_PROTOCOL_VERSION, MAX_POLICY_SETTINGS, PolicyDrift,
    policy_drift,
};

use crate::{TaskStore, TaskStoreError};

/// Matches the catalog's lifetime: a policy snapshot is a short-lived pull, not
/// a grant, and re-fetching is cheap.
const POLICY_SNAPSHOT_LIFETIME_SECONDS: i64 = 300;

pub(super) fn migrate(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_policy_settings (
            apiary_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (apiary_id, key)
         );
         CREATE TABLE IF NOT EXISTS local_policy_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            applied_at INTEGER NOT NULL CHECK(applied_at >= 0)
         );
         CREATE TABLE IF NOT EXISTS local_policy_manifest (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            policy_revision INTEGER NOT NULL CHECK(policy_revision > 0),
            settings_digest TEXT NOT NULL,
            settings_json TEXT NOT NULL,
            received_at INTEGER NOT NULL CHECK(received_at >= 0)
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::APIARY_POLICY_SCHEMA_MARKER)
}

fn settings_digest(settings: &[ApiaryPolicySetting]) -> Result<String, TaskStoreError> {
    let canonical = serde_json::to_vec(settings)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.apiary-policy-settings.v1\0");
    digest.update(canonical);
    Ok(Base64UrlUnpadded::encode_string(&digest.finalize()))
}

fn canonical_policy_payload(
    payload: &ApiaryPolicySnapshotPayload,
) -> Result<Vec<u8>, TaskStoreError> {
    serde_json::to_vec(payload).map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
}

/// How a member stands against the Apiary defaults it holds.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PolicyConvergence {
    /// The revision whose settings this member holds, if any.
    pub policy_revision: Option<u64>,
    /// The Apiary defaults as delivered.
    pub expected: Vec<ApiaryPolicySetting>,
    /// Where this member differs. Empty means converged.
    pub drift: Vec<PolicyDrift>,
    /// True when this member has accepted a revision but holds no settings body
    /// for it.
    ///
    /// ⚠️ THIS IS THE PRE-BODY MEMBER, and it is a real state rather than an
    /// error. Members joined and accepted revisions while policy was a bare
    /// integer. Their acceptance was genuine and MUST NOT be invalidated — doing
    /// so would fail their membership closed for a change they had no part in.
    /// They simply have nothing to converge to yet.
    pub awaiting_body: bool,
}

impl TaskStore {
    /// The Apiary one authenticated member node belongs to.
    ///
    /// Exists so a caller can authenticate a member WITHOUT being handed the
    /// credential-matching internals. Used by the event socket, which must
    /// recheck authority at connect time rather than trust an upgrade.
    ///
    /// # Errors
    /// Rejects malformed, unknown and expired credentials.
    pub fn authenticated_member_apiary(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<swarm_domain::ApiaryId, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let credential: [u8; 32] = Base64UrlUnpadded::decode_vec(credential)
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = crate::federation::authenticate_member_credential(
            &transaction,
            &identity,
            &credential,
            now,
        )?;
        transaction.commit()?;
        Ok(member.apiary)
    }

    /// Keeper replacing the Apiary's shared defaults, which advances the policy
    /// revision.
    ///
    /// ⚠️ ADVANCING THE REVISION IS THE POINT AND ALSO THE COST. Acceptance is
    /// revision-bound: an invitation records the revision it requires, and a
    /// changed revision makes prior acceptance stale so JOINING fails closed.
    /// That is existing, deliberate behaviour for invitations in flight. It does
    /// NOT retroactively unmake an existing membership, and this must never be
    /// used to force one closed.
    ///
    /// # Errors
    /// Rejects non-Keepers, invalid or forbidden settings, and unavailable
    /// persistence.
    pub fn set_apiary_policy_settings(
        &self,
        settings: &[ApiaryPolicySetting],
        now: i64,
    ) -> Result<u64, TaskStoreError> {
        if now < 0 || settings.len() > MAX_POLICY_SETTINGS {
            return Err(TaskStoreError::InvalidApiary);
        }
        if !settings.iter().all(ApiaryPolicySetting::is_valid) {
            return Err(TaskStoreError::InvalidApiary);
        }
        let mut keys: Vec<&str> = settings.iter().map(|s| s.key.as_str()).collect();
        keys.sort_unstable();
        let total = keys.len();
        keys.dedup();
        if keys.len() != total {
            return Err(TaskStoreError::InvalidApiary);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let apiary_id: String = transaction
            .query_row(
                "SELECT id FROM apiaries
                 WHERE keeper_operator_id = ?1 AND collapsed_at IS NULL",
                [identity.operator.id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidApiary)?;
        transaction.execute(
            "DELETE FROM apiary_policy_settings WHERE apiary_id = ?1",
            [&apiary_id],
        )?;
        for setting in settings {
            transaction.execute(
                "INSERT INTO apiary_policy_settings (apiary_id, key, value) VALUES (?1, ?2, ?3)",
                params![&apiary_id, setting.key, setting.value],
            )?;
        }
        transaction.execute(
            "UPDATE apiaries SET policy_revision = policy_revision + 1, updated_at = ?2
             WHERE id = ?1",
            params![&apiary_id, now],
        )?;
        let revision: u64 = transaction.query_row(
            "SELECT policy_revision FROM apiaries WHERE id = ?1",
            [&apiary_id],
            |row| row.get(0),
        )?;
        crate::insert_control_room_event(
            &transaction,
            swarm_domain::ControlRoomEventKind::RuntimeChanged,
        )?;
        transaction.commit()?;
        Ok(revision)
    }

    /// The Apiary's current shared defaults, as Keeper holds them.
    ///
    /// # Errors
    /// Returns storage failures.
    pub fn apiary_policy_settings(&self) -> Result<Vec<ApiaryPolicySetting>, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT key, value FROM apiary_policy_settings
             WHERE apiary_id = (SELECT id FROM apiaries
                WHERE keeper_operator_id = ?1 AND collapsed_at IS NULL)
             ORDER BY key",
        )?;
        let rows = statement.query_map([identity.operator.id.to_string()], |row| {
            Ok(ApiaryPolicySetting {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Keeper signing the current policy body for one authenticated member.
    ///
    /// # Errors
    /// Rejects invalid credentials and unavailable persistence.
    pub fn signed_apiary_policy(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<ApiaryPolicySnapshot, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let credential: [u8; 32] = Base64UrlUnpadded::decode_vec(credential)
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        let identity = self.local_hive_identity()?;
        let local_node = self.local_federation_identity(now)?;
        let settings = self.apiary_policy_settings()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = crate::federation::authenticate_member_credential(
            &transaction,
            &identity,
            &credential,
            now,
        )?;
        let policy_revision: u64 = transaction.query_row(
            "SELECT policy_revision FROM apiaries WHERE id = ?1",
            [member.apiary.to_string()],
            |row| row.get(0),
        )?;
        let payload = ApiaryPolicySnapshotPayload {
            schema_version: APIARY_POLICY_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            apiary_id: member.apiary,
            policy_revision,
            settings_digest: settings_digest(&settings)?,
            settings,
            keeper_node_id: local_node.node_id,
            keeper_hive_id: identity.hive.id,
            keeper_operator_id: identity.operator.id,
            member_node_id: member.node,
            issued_at: now,
            expires_at: now
                .checked_add(POLICY_SNAPSHOT_LIFETIME_SECONDS)
                .ok_or(TaskStoreError::InvalidFederationCredential)?,
        };
        if !payload.is_valid() {
            return Err(TaskStoreError::InvalidApiary);
        }
        let signature = local_node
            .signing_key
            .sign(&canonical_policy_payload(&payload)?);
        transaction.commit()?;
        Ok(ApiaryPolicySnapshot {
            payload,
            signature: Base64UrlUnpadded::encode_string(&signature.to_bytes()),
        })
    }

    /// A member verifying and storing the Apiary's current defaults.
    ///
    /// ⚠️ STORING IS NOT APPLYING. This records what the Apiary expects; it does
    /// not change a single local setting. Convergence is the member's own act,
    /// which is what keeps this shared defaults rather than remote control.
    ///
    /// # Errors
    /// Rejects non-members, bad signatures, wrong scope, expired snapshots, a
    /// digest that does not match its settings, and unavailable persistence.
    pub fn apply_apiary_policy(
        &self,
        snapshot: &ApiaryPolicySnapshot,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.require_local_federation_member()?;
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (receipt, key): (String, String) = transaction.query_row(
            "SELECT m.receipt_json, i.keeper_public_key FROM local_federation_membership m
             JOIN apiary_join_invitations i ON i.id = m.invitation_id
             WHERE m.singleton = 1 AND m.state = 'active' AND m.credential_expires_at > ?1",
            [now],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let receipt: swarm_domain::FederationMembershipReceipt = serde_json::from_str(&receipt)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let payload = &snapshot.payload;
        if payload.schema_version != APIARY_POLICY_SCHEMA_VERSION
            || payload.protocol_version != FEDERATION_PROTOCOL_VERSION
            || payload.apiary_id != receipt.payload.apiary_id
            || payload.keeper_node_id != receipt.payload.keeper_node_id
            || payload.member_node_id != receipt.payload.member_node_id
            || payload.expires_at <= now
            || !payload.is_valid()
            || payload.settings_digest != settings_digest(&payload.settings)?
        {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        if identity.hive.apiary_id != Some(payload.apiary_id) {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let verifying: [u8; 32] = Base64UrlUnpadded::decode_vec(&key)
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&snapshot.signature)
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?
            .try_into()
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        // Canonicalise BEFORE entering the signature closure: its error type is
        // ed25519's, not ours, and threading one through the other buys nothing.
        let canonical = canonical_policy_payload(payload)?;
        VerifyingKey::from_bytes(&verifying)
            .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
            .map_err(|_| TaskStoreError::InvalidFederationCredential)?;
        let prior: Option<(u64, String)> = transaction
            .query_row(
                "SELECT policy_revision, settings_digest FROM local_policy_manifest
                 WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((revision, digest)) = &prior {
            if *revision == payload.policy_revision && digest == &payload.settings_digest {
                return Ok(false);
            }
            if *revision > payload.policy_revision {
                return Err(TaskStoreError::InvalidHiveIdentity);
            }
        }
        let serialized = serde_json::to_string(&payload.settings)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        transaction.execute(
            "INSERT INTO local_policy_manifest
                (singleton, policy_revision, settings_digest, settings_json, received_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(singleton) DO UPDATE SET
                policy_revision = excluded.policy_revision,
                settings_digest = excluded.settings_digest,
                settings_json = excluded.settings_json,
                received_at = excluded.received_at",
            params![
                payload.policy_revision,
                payload.settings_digest,
                serialized,
                now
            ],
        )?;
        crate::insert_control_room_event(
            &transaction,
            swarm_domain::ControlRoomEventKind::RuntimeChanged,
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// A member recording that it has applied a shared default locally.
    ///
    /// # Errors
    /// Rejects invalid settings and unavailable persistence.
    pub fn record_local_policy_setting(
        &self,
        setting: &ApiaryPolicySetting,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        if now < 0 || !setting.is_valid() {
            return Err(TaskStoreError::InvalidApiary);
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO local_policy_settings (key, value, applied_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, applied_at = excluded.applied_at",
            params![setting.key, setting.value, now],
        )?;
        Ok(())
    }

    /// What this member actually runs, for the settings it has applied.
    ///
    /// # Errors
    /// Returns storage failures.
    pub fn local_policy_settings(&self) -> Result<Vec<ApiaryPolicySetting>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT key, value FROM local_policy_settings ORDER BY key")?;
        let rows = statement.query_map([], |row| {
            Ok(ApiaryPolicySetting {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Where this member stands against the Apiary's defaults.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a false convergence.
    pub fn local_policy_convergence(&self) -> Result<PolicyConvergence, TaskStoreError> {
        let connection = self.connection()?;
        let manifest: Option<(u64, String)> = connection
            .query_row(
                "SELECT policy_revision, settings_json FROM local_policy_manifest
                 WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        drop(connection);
        let applied = self.local_policy_settings()?;
        let Some((policy_revision, settings)) = manifest else {
            // Accepted a revision, holds no body for it. The pre-body member:
            // a real state, not an error, and nothing to converge to yet.
            return Ok(PolicyConvergence {
                policy_revision: None,
                expected: Vec::new(),
                drift: Vec::new(),
                awaiting_body: true,
            });
        };
        let expected: Vec<ApiaryPolicySetting> = serde_json::from_str(&settings)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let drift = policy_drift(&expected, &applied);
        Ok(PolicyConvergence {
            policy_revision: Some(policy_revision),
            expected,
            drift,
            awaiting_body: false,
        })
    }

    /// Keeper's view of one member's convergence, from the capability report it
    /// already publishes. Returns the Apiary defaults for comparison.
    ///
    /// # Errors
    /// Returns storage failures.
    pub fn apiary_policy_expectation(
        &self,
    ) -> Result<(u64, Vec<ApiaryPolicySetting>), TaskStoreError> {
        let settings = self.apiary_policy_settings()?;
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let revision: u64 = connection
            .query_row(
                "SELECT policy_revision FROM apiaries
                 WHERE keeper_operator_id = ?1 AND collapsed_at IS NULL",
                [identity.operator.id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidApiary)?;
        Ok((revision, settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(key: &str, value: &str) -> ApiaryPolicySetting {
        ApiaryPolicySetting {
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }

    fn defaults() -> Vec<ApiaryPolicySetting> {
        vec![
            setting("checks.max_warnings", "0"),
            setting("checks.required", "true"),
            setting("workflow.review_required", "true"),
        ]
    }

    /// The whole loop: Keeper authors defaults, a member receives the signed
    /// body, and its gap from them is computable.
    #[test]
    fn a_revision_carries_content_and_a_member_can_see_its_gap() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;

        let revision = keeper
            .set_apiary_policy_settings(&defaults(), now + 5)
            .unwrap();
        assert!(
            revision > 1,
            "authoring defaults advances the policy revision"
        );

        let snapshot = keeper.signed_apiary_policy(&credential, now + 6).unwrap();
        assert_eq!(snapshot.payload.policy_revision, revision);
        assert_eq!(snapshot.payload.settings, defaults());
        assert!(member.apply_apiary_policy(&snapshot, now + 7).unwrap());

        // Received, but nothing applied yet: every default reads as not applied,
        // which is exactly what a member that has just been handed policy looks
        // like.
        let convergence = member.local_policy_convergence().unwrap();
        assert!(!convergence.awaiting_body);
        assert_eq!(convergence.policy_revision, Some(revision));
        assert_eq!(convergence.drift.len(), 3);
        assert!(convergence.drift.iter().all(|entry| entry.local.is_none()));

        // Converge on two, override the third.
        member
            .record_local_policy_setting(&setting("checks.required", "true"), now + 8)
            .unwrap();
        member
            .record_local_policy_setting(&setting("workflow.review_required", "true"), now + 8)
            .unwrap();
        member
            .record_local_policy_setting(&setting("checks.max_warnings", "5"), now + 8)
            .unwrap();

        let convergence = member.local_policy_convergence().unwrap();
        assert_eq!(convergence.drift.len(), 1, "only the override remains");
        assert_eq!(convergence.drift[0].key, "checks.max_warnings");
        assert_eq!(convergence.drift[0].expected, "0");
        assert_eq!(
            convergence.drift[0].local.as_deref(),
            Some("5"),
            "an override is REPORTED, not prevented -- the member still runs 5"
        );
    }

    /// ⚠️ THE PRE-BODY MEMBER, WHICH THE ACCEPTANCE ASKS FOR BY NAME. Members
    /// joined and accepted revisions while policy was a bare integer. Their
    /// acceptance was genuine and must NOT be invalidated: doing so would fail
    /// an existing membership closed for a change they had no part in. They
    /// simply have nothing to converge to until a body arrives.
    #[test]
    fn a_member_that_accepted_an_empty_revision_is_awaiting_a_body_not_broken() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);

        let convergence = member.local_policy_convergence().unwrap();
        assert!(convergence.awaiting_body, "no body has ever been published");
        assert_eq!(convergence.policy_revision, None);
        assert!(convergence.drift.is_empty(), "nothing to deviate from yet");

        // Membership is untouched and still usable.
        assert!(member.federation_member_connection().is_ok());

        // And the moment a body exists, the same member converges normally.
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        keeper
            .set_apiary_policy_settings(&defaults(), now + 5)
            .unwrap();
        let snapshot = keeper.signed_apiary_policy(&credential, now + 6).unwrap();
        assert!(member.apply_apiary_policy(&snapshot, now + 7).unwrap());
        assert!(!member.local_policy_convergence().unwrap().awaiting_body);
    }

    /// Storing the Apiary's expectation must not change a single local setting.
    /// If it did, this would be remote configuration rather than shared defaults.
    #[test]
    fn receiving_policy_applies_nothing_by_itself() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        member
            .record_local_policy_setting(&setting("checks.max_warnings", "5"), now)
            .unwrap();

        keeper
            .set_apiary_policy_settings(&defaults(), now + 5)
            .unwrap();
        let snapshot = keeper.signed_apiary_policy(&credential, now + 6).unwrap();
        member.apply_apiary_policy(&snapshot, now + 7).unwrap();

        assert_eq!(
            member.local_policy_settings().unwrap(),
            vec![setting("checks.max_warnings", "5")],
            "the member still runs exactly what it ran before"
        );
    }

    /// ⚠️ THE EXCLUSION DOC 98 KEEPS. Refused at the Keeper, not merely at the
    /// type: nothing that reaches for a credential, a filesystem root or a
    /// provider permission can be authored as policy in the first place.
    #[test]
    fn keeper_cannot_author_credentials_roots_or_provider_permissions() {
        let now = 120_000;
        let (keeper, _member) = crate::federation::tests::joined_member(now);
        for forbidden in [
            "credentials.jira",
            "workspace.root",
            "provider.permission.bash",
        ] {
            assert!(
                keeper
                    .set_apiary_policy_settings(&[setting(forbidden, "x")], now + 5)
                    .is_err(),
                "{forbidden:?} must not be distributable as policy"
            );
        }
        // And the refusal is atomic: nothing was written on the way out.
        assert!(keeper.apiary_policy_settings().unwrap().is_empty());
    }

    #[test]
    fn a_tampered_or_stale_policy_body_fails_closed() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        keeper
            .set_apiary_policy_settings(&defaults(), now + 5)
            .unwrap();
        let snapshot = keeper.signed_apiary_policy(&credential, now + 6).unwrap();

        let mut tampered = snapshot.clone();
        tampered
            .payload
            .settings
            .push(setting("checks.injected", "true"));
        assert!(
            member.apply_apiary_policy(&tampered, now + 7).is_err(),
            "an altered manifest must not verify"
        );

        // Expired snapshots are refused: this is a short-lived pull, not a grant.
        assert!(
            member
                .apply_apiary_policy(&snapshot, now + 6 + POLICY_SNAPSHOT_LIFETIME_SECONDS + 1)
                .is_err()
        );

        // The genuine one still applies, and an identical retry is free.
        assert!(member.apply_apiary_policy(&snapshot, now + 7).unwrap());
        assert!(!member.apply_apiary_policy(&snapshot, now + 8).unwrap());
    }
}
