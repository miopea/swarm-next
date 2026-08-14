use std::str::FromStr;

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ApiaryHiveCandidate, ApiaryId, FEDERATION_CONNECTION_CARD_SCHEMA_VERSION,
    FEDERATION_PROTOCOL_VERSION, FederationNodeId, HiveConnectionCard, HiveConnectionCardPayload,
    HiveId,
};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

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
}
