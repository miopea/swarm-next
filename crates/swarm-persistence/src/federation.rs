use std::str::FromStr;

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    FEDERATION_CONNECTION_CARD_SCHEMA_VERSION, FEDERATION_PROTOCOL_VERSION, FederationNodeId,
    HiveConnectionCard, HiveConnectionCardPayload,
};

use crate::{TaskStore, TaskStoreError};

pub const MIN_CONNECTION_CARD_LIFETIME_SECONDS: i64 = 5 * 60;
pub const MAX_CONNECTION_CARD_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_CLOCK_SKEW_SECONDS: i64 = 5 * 60;

struct LocalFederationIdentity {
    node_id: FederationNodeId,
    signing_key: SigningKey,
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
        let payload = HiveConnectionCardPayload {
            schema_version: FEDERATION_CONNECTION_CARD_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            node_id: local_node.node_id,
            hive_id: identity.hive.id,
            hive_name: identity.hive.name,
            operator_id: identity.operator.id,
            operator_display_name: identity.operator.display_name,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
