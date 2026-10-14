//! Membership renewal (ADR 0117): a member trades its credential for a fresh
//! one before it lapses, and the Keeper re-signs the same membership receipt
//! with the new expiry. Without it every member fell out of its Apiary thirty
//! days after joining.

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::Signer;
use rusqlite::{OptionalExtension, params};
use swarm_domain::{FederationJoinAcceptance, FederationMembershipReceipt, membership_renewable};

use crate::{
    TaskStore, TaskStoreError,
    federation::{
        FEDERATION_NODE_CREDENTIAL_LIFETIME_SECONDS, canonical_membership_receipt_payload,
        decode_node_credential, invitation_secret_digest, node_credential_material,
        verify_federation_membership_receipt,
    },
};

/// The Keeper remembers the credential a renewal replaced, so a member whose
/// response was lost can ask again with it. Both sides let the credential,
/// receipt and expiry change; who and what the membership is stays frozen.
///
/// ⚠️ A RECEIPT IS REPLACED ONLY BY ONE FOR THE SAME MEMBERSHIP. Renewal has
/// to rewrite it, so it can no longer be frozen outright, but anything that is
/// not the same receipt id and join date (and, on the Keeper, the same member)
/// is still refused by the database itself, whatever code asks.
pub(crate) fn migrate_membership_renewal(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    if !crate::federation::federation_column_exists(
        transaction,
        "apiary_federation_memberships",
        "previous_credential_digest",
    )? {
        transaction.execute_batch(
            "ALTER TABLE apiary_federation_memberships ADD COLUMN previous_credential_digest BLOB;",
        )?;
    }
    transaction.execute_batch(
        "DROP TRIGGER IF EXISTS immutable_apiary_federation_membership;
         CREATE TRIGGER immutable_apiary_federation_membership
             BEFORE UPDATE OF receipt_id, invitation_id, apiary_id,
                 member_node_id, member_hive_id, member_operator_id, joined_at
             ON apiary_federation_memberships
             BEGIN SELECT RAISE(ABORT, 'Federation membership identity is immutable'); END;
         DROP TRIGGER IF EXISTS renewed_apiary_federation_receipt;
         CREATE TRIGGER renewed_apiary_federation_receipt
             BEFORE UPDATE OF receipt_json ON apiary_federation_memberships
             WHEN json_extract(NEW.receipt_json, '$.payload.receipt_id') IS NOT OLD.receipt_id
               OR json_extract(NEW.receipt_json, '$.payload.member_hive_id') IS NOT OLD.member_hive_id
               OR json_extract(NEW.receipt_json, '$.payload.joined_at') IS NOT OLD.joined_at
             BEGIN SELECT RAISE(ABORT, 'Federation membership identity is immutable'); END;
         DROP TRIGGER IF EXISTS immutable_local_federation_membership;
         CREATE TRIGGER immutable_local_federation_membership
             BEFORE UPDATE OF receipt_id, invitation_id, apiary_id, keeper_node_id, joined_at
             ON local_federation_membership
             BEGIN SELECT RAISE(ABORT, 'Local federation membership identity is immutable'); END;
         DROP TRIGGER IF EXISTS renewed_local_federation_receipt;
         CREATE TRIGGER renewed_local_federation_receipt
             BEFORE UPDATE OF receipt_json ON local_federation_membership
             WHEN json_extract(NEW.receipt_json, '$.payload.receipt_id') IS NOT OLD.receipt_id
               OR json_extract(NEW.receipt_json, '$.payload.joined_at') IS NOT OLD.joined_at
             BEGIN SELECT RAISE(ABORT, 'Local federation membership identity is immutable'); END;",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::MEMBERSHIP_RENEWAL_SCHEMA_MARKER,
    )
}

fn acceptance_from(
    receipt_json: &str,
    credential: &[u8],
) -> Result<FederationJoinAcceptance, TaskStoreError> {
    Ok(FederationJoinAcceptance {
        receipt: serde_json::from_str(receipt_json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
        node_credential: Base64UrlUnpadded::encode_string(credential),
    })
}

impl TaskStore {
    /// Keeper: issues a member a fresh credential and its membership receipt
    /// re-signed with the new expiry (ADR 0117).
    ///
    /// The credential it replaces can ask again and gets the same answer, so a
    /// lost response never locks a member out. An expired credential can renew
    /// for the grace period and do nothing else; a removed member cannot.
    ///
    /// # Errors
    /// Rejects a credential that is unknown, past its grace period, or not an
    /// active member of this Keeper's Apiary, and returns storage failures.
    pub fn renew_federation_membership(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationJoinAcceptance, TaskStoreError> {
        let credential = decode_node_credential(node_credential)?;
        let identity = self.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::InvalidFederationCredential)?;
        let local_node = self.local_federation_identity(now)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let keeper: Option<String> = transaction
            .query_row(
                "SELECT keeper_operator_id FROM apiaries WHERE id = ?1 AND collapsed_at IS NULL",
                [apiary_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if keeper.as_deref() != Some(identity.operator.id.to_string().as_str()) {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let digest = invitation_secret_digest(&credential);
        // A retry with the credential a renewal already replaced: the response
        // was lost, so the member gets the same one again.
        let replay: Option<(String, Vec<u8>)> = transaction
            .query_row(
                "SELECT receipt_json, node_credential FROM apiary_federation_memberships
                 WHERE apiary_id = ?1 AND previous_credential_digest = ?2
                   AND state = 'active' AND credential_expires_at > ?3",
                params![apiary_id.to_string(), digest.as_slice(), now],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((receipt_json, issued)) = replay {
            return acceptance_from(&receipt_json, &issued);
        }
        let (receipt_id, member_hive, receipt_json, expires_at): (String, String, String, i64) =
            transaction
                .query_row(
                    "SELECT receipt_id, member_hive_id, receipt_json, credential_expires_at
                     FROM apiary_federation_memberships
                     WHERE apiary_id = ?1 AND credential_digest = ?2 AND state = 'active'",
                    params![apiary_id.to_string(), digest.as_slice()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?
                .ok_or(TaskStoreError::InvalidFederationCredential)?;
        if !membership_renewable(expires_at, now) {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let mut receipt: FederationMembershipReceipt = serde_json::from_str(&receipt_json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        if receipt.payload.keeper_node_id != local_node.node_id {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        let (fresh, encoded) = node_credential_material()?;
        let renewed_until = now
            .checked_add(FEDERATION_NODE_CREDENTIAL_LIFETIME_SECONDS)
            .ok_or(TaskStoreError::InvalidFederationCredential)?;
        receipt.payload.credential_expires_at = renewed_until;
        receipt.payload.credential_issued_at = Some(now);
        receipt.signature = Base64UrlUnpadded::encode_string(
            &local_node
                .signing_key
                .sign(&canonical_membership_receipt_payload(&receipt.payload)?)
                .to_bytes(),
        );
        let receipt_json = serde_json::to_string(&receipt)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let changed = transaction.execute(
            "UPDATE apiary_federation_memberships
             SET previous_credential_digest = credential_digest,
                 credential_digest = ?3, node_credential = ?4,
                 credential_expires_at = ?5, receipt_json = ?6, last_contact_at = ?7
             WHERE receipt_id = ?1 AND credential_digest = ?2 AND state = 'active'",
            params![
                receipt_id,
                digest.as_slice(),
                invitation_secret_digest(&fresh).as_slice(),
                fresh.as_slice(),
                renewed_until,
                receipt_json,
                now
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        crate::hive_health::record_contact_slot(
            &transaction,
            &apiary_id.to_string(),
            &member_hive,
            now,
        )?;
        crate::insert_control_room_event(
            &transaction,
            swarm_domain::ControlRoomEventKind::RuntimeChanged,
        )?;
        transaction.commit()?;
        Ok(FederationJoinAcceptance {
            receipt,
            node_credential: encoded,
        })
    }

    /// Member: keeps the credential and receipt its Keeper renewed (ADR 0117),
    /// once they prove to be the same membership signed by the same Keeper,
    /// lasting longer than the one held now.
    ///
    /// # Errors
    /// Rejects a renewal for another membership, an older or unsigned one, and
    /// a member that is not active, and returns storage failures.
    pub fn apply_federation_membership_renewal(
        &self,
        renewal: &FederationJoinAcceptance,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let credential = decode_node_credential(&renewal.node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (receipt_json, key, held_until): (String, String, i64) = transaction
            .query_row(
                "SELECT m.receipt_json, i.keeper_public_key, m.credential_expires_at
                 FROM local_federation_membership m
                 JOIN apiary_join_invitations i ON i.id = m.invitation_id
                 WHERE m.singleton = 1 AND m.state = 'active'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationCredential)?;
        let held: FederationMembershipReceipt = serde_json::from_str(&receipt_json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        verify_federation_membership_receipt(&renewal.receipt, &key, now)?;
        let (held, renewed) = (&held.payload, &renewal.receipt.payload);
        if renewed.receipt_id != held.receipt_id
            || renewed.invitation_id != held.invitation_id
            || renewed.apiary_id != held.apiary_id
            || renewed.keeper_node_id != held.keeper_node_id
            || renewed.keeper_hive_id != held.keeper_hive_id
            || renewed.member_node_id != held.member_node_id
            || renewed.member_hive_id != held.member_hive_id
            || renewed.member_operator_id != held.member_operator_id
            || renewed.joined_at != held.joined_at
            || renewed.credential_expires_at <= held_until
        {
            return Err(TaskStoreError::InvalidFederationCredential);
        }
        transaction.execute(
            "UPDATE local_federation_membership
             SET receipt_json = ?1, node_credential = ?2, credential_digest = ?3,
                 credential_expires_at = ?4
             WHERE singleton = 1 AND state = 'active'",
            params![
                serde_json::to_string(&renewal.receipt)
                    .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
                credential.as_slice(),
                invitation_secret_digest(&credential).as_slice(),
                renewed.credential_expires_at,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }
}
