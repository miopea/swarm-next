//! Local public profile ownership. Federation projection lives separately from
//! private Hive identity and must never grant membership or execution authority.

use base64ct::{Base64UrlUnpadded, Encoding};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{ControlRoomEventKind, PublicHiveProfile};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalPublicHiveProfile {
    pub profile: PublicHiveProfile,
    pub revision: u64,
}

pub(crate) fn migrate(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_public_hive_profile (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            revision INTEGER NOT NULL CHECK (revision > 0),
            contact_email TEXT
        );
        INSERT INTO local_public_hive_profile (singleton, revision) VALUES (1, 1)
        ON CONFLICT(singleton) DO NOTHING;",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        super::PUBLIC_HIVE_PROFILE_SCHEMA_VERSION,
    )?;
    Ok(())
}

pub(crate) fn migrate_shared_profiles(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS federation_public_profiles (
        apiary_id TEXT NOT NULL REFERENCES apiaries(id),
        node_id TEXT NOT NULL,
        revision INTEGER NOT NULL CHECK (revision > 0),
        payload_json TEXT NOT NULL,
        PRIMARY KEY (apiary_id, node_id)
    );",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        super::FEDERATION_PUBLIC_PROFILES_SCHEMA_VERSION,
    )?;
    Ok(())
}

pub(crate) fn advance_local_profile_revision(
    connection: &Connection,
) -> Result<(), TaskStoreError> {
    if connection.execute(
        "UPDATE local_public_hive_profile SET revision = revision + 1
         WHERE singleton = 1 AND revision < 9223372036854775807",
        [],
    )? != 1
    {
        return Err(TaskStoreError::InvalidHiveIdentity);
    }
    Ok(())
}

pub(crate) fn migrate_directory(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_directory_revisions (
        apiary_id TEXT PRIMARY KEY REFERENCES apiaries(id),
        revision INTEGER NOT NULL CHECK (revision > 0),
        entries_json TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS local_apiary_directory (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        apiary_id TEXT NOT NULL,
        revision INTEGER NOT NULL CHECK (revision > 0),
        payload_json TEXT NOT NULL
    );",
    )?;
    transaction.pragma_update(None, "user_version", super::APIARY_DIRECTORY_SCHEMA_VERSION)?;
    Ok(())
}

pub(crate) fn directory_entries(
    connection: &Connection,
    keeper: swarm_domain::PublicHiveIdentity,
    apiary_id: swarm_domain::ApiaryId,
) -> Result<Vec<swarm_domain::ApiaryDirectoryEntry>, TaskStoreError> {
    let local = read_profile(connection)?;
    let mut entries = vec![swarm_domain::ApiaryDirectoryEntry {
        identity: keeper,
        profile: local.profile,
        profile_revision: local.revision,
        role: swarm_domain::LocalApiaryRole::Keeper,
    }];
    let mut statement = connection.prepare("SELECT m.member_node_id, m.member_hive_id, m.member_operator_id,
        h.name, o.display_name, p.payload_json FROM apiary_federation_memberships m
        JOIN hives h ON h.id = m.member_hive_id AND h.apiary_id = m.apiary_id
        JOIN operators o ON o.id = m.member_operator_id
        LEFT JOIN federation_public_profiles p ON p.apiary_id = m.apiary_id AND p.node_id = m.member_node_id
        WHERE m.apiary_id = ?1 AND m.state = 'active' ORDER BY m.member_node_id LIMIT 257")?;
    let rows = statement.query_map([apiary_id.to_string()], |row| {
        Ok((
            swarm_domain::PublicHiveIdentity {
                node_id: super::parse_domain_id(&row.get::<_, String>(0)?)?,
                hive_id: super::parse_domain_id(&row.get::<_, String>(1)?)?,
                operator_id: super::parse_domain_id(&row.get::<_, String>(2)?)?,
            },
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    for row in rows {
        let (identity, hive_name, operator_display_name, saved) = row?;
        let (profile, profile_revision) = if let Some(saved) = saved {
            let update: swarm_domain::FederationProfileUpdatePayload = serde_json::from_str(&saved)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            if !update.matches_member(apiary_id, identity) {
                return Err(TaskStoreError::InvalidHiveIdentity);
            }
            (update.profile, update.revision)
        } else {
            (
                PublicHiveProfile {
                    hive_name,
                    operator_display_name,
                    contact_email: None,
                },
                1,
            )
        };
        entries.push(swarm_domain::ApiaryDirectoryEntry {
            identity,
            profile,
            profile_revision,
            role: swarm_domain::LocalApiaryRole::Member,
        });
        if entries.len() > swarm_domain::MAX_APIARY_DIRECTORY_ENTRIES {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
    }
    Ok(entries)
}

pub(crate) fn directory_revision(
    connection: &Connection,
    apiary_id: swarm_domain::ApiaryId,
    entries: &[swarm_domain::ApiaryDirectoryEntry],
) -> Result<u64, TaskStoreError> {
    let serialized = serde_json::to_string(entries)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    let prior = connection
        .query_row(
            "SELECT revision, entries_json FROM apiary_directory_revisions WHERE apiary_id = ?1",
            [apiary_id.to_string()],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let revision = match prior {
        Some((revision, saved)) if saved == serialized => return Ok(revision),
        Some((revision, _)) => revision
            .checked_add(1)
            .filter(|next| i64::try_from(*next).is_ok())
            .ok_or(TaskStoreError::InvalidHiveIdentity)?,
        None => 1,
    };
    connection.execute("INSERT INTO apiary_directory_revisions (apiary_id, revision, entries_json) VALUES (?1, ?2, ?3)
        ON CONFLICT(apiary_id) DO UPDATE SET revision = excluded.revision, entries_json = excluded.entries_json",
        params![apiary_id.to_string(), revision, serialized])?;
    Ok(revision)
}

fn read_profile(connection: &Connection) -> Result<LocalPublicHiveProfile, TaskStoreError> {
    connection
        .query_row(
            "SELECT h.name, o.display_name, p.contact_email, p.revision
         FROM local_hive_identity l JOIN hives h ON h.id = l.hive_id
         JOIN operators o ON o.id = h.operator_id
         JOIN local_public_hive_profile p ON p.singleton = l.singleton
         WHERE l.singleton = 1",
            [],
            |row| {
                Ok(LocalPublicHiveProfile {
                    profile: PublicHiveProfile {
                        hive_name: row.get(0)?,
                        operator_display_name: row.get(1)?,
                        contact_email: row.get(2)?,
                    },
                    revision: row.get(3)?,
                })
            },
        )
        .map_err(Into::into)
}

impl TaskStore {
    /// Accepts only an active member's pinned-key-signed public labels. Revision
    /// and label changes commit atomically. Identical retries do not emit events.
    ///
    /// # Errors
    /// Rejects invalid credentials/signatures, wrong scope, stale/conflicting
    /// revisions, capacity overflow, or unavailable persistence.
    pub fn accept_federation_public_profile(
        &self,
        credential: &str,
        update: &swarm_domain::FederationProfileUpdate,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
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
        let member = super::federation::authenticate_member_credential(
            &transaction,
            &identity,
            &credential,
            now,
        )?;
        let key: String = transaction.query_row(
            "SELECT public_key FROM apiary_hive_candidates WHERE apiary_id = ?1 AND node_id = ?2 AND hive_id = ?3 AND operator_id = ?4",
            params![member.apiary.to_string(), member.node.to_string(), member.hive.to_string(), member.operator.to_string()],
            |row| row.get(0),
        )?;
        super::verify_federation_profile_update(
            update,
            member.apiary,
            swarm_domain::PublicHiveIdentity {
                node_id: member.node,
                hive_id: member.hive,
                operator_id: member.operator,
            },
            &key,
        )?;
        let payload = serde_json::to_string(&update.payload)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let prior = transaction.query_row(
            "SELECT revision, payload_json FROM federation_public_profiles WHERE apiary_id = ?1 AND node_id = ?2",
            params![member.apiary.to_string(), member.node.to_string()],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        ).optional()?;
        if let Some((revision, saved)) = prior {
            if revision == update.payload.revision && saved == payload {
                return Ok(false);
            }
            if revision >= update.payload.revision {
                return Err(TaskStoreError::InvalidHiveIdentity);
            }
        } else {
            let count: usize = transaction.query_row(
                "SELECT count(*) FROM federation_public_profiles WHERE apiary_id = ?1",
                [member.apiary.to_string()],
                |row| row.get(0),
            )?;
            if count >= swarm_domain::MAX_APIARY_DIRECTORY_ENTRIES {
                return Err(TaskStoreError::InvalidHiveIdentity);
            }
        }
        transaction.execute("INSERT INTO federation_public_profiles (apiary_id, node_id, revision, payload_json) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(apiary_id, node_id) DO UPDATE SET revision = excluded.revision, payload_json = excluded.payload_json",
            params![member.apiary.to_string(), member.node.to_string(), update.payload.revision, payload])?;
        if transaction.execute("UPDATE hives SET name = ?1, updated_at = ?2 WHERE id = ?3 AND operator_id = ?4 AND apiary_id = ?5",
            params![update.payload.profile.hive_name, now, member.hive.to_string(), member.operator.to_string(), member.apiary.to_string()])? != 1 {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        transaction.execute(
            "UPDATE operators SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                update.payload.profile.operator_display_name,
                now,
                member.operator.to_string()
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        Ok(true)
    }

    /// Returns the local owner's public labels and monotonic revision, not
    /// authentication or inferred data from another integration.
    ///
    /// # Errors
    /// Returns persistence or local identity errors.
    pub fn local_public_hive_profile(&self) -> Result<LocalPublicHiveProfile, TaskStoreError> {
        let connection = self.connection()?;
        read_profile(&connection)
    }

    /// Saves only local display fields. The revision and notification commit
    /// atomically; workers, membership, keys and permission grants are unchanged.
    /// Identical saves preserve the revision. Join default naming is explicit
    /// upstream and is never applied by this general-purpose save operation.
    ///
    /// # Errors
    /// Rejects invalid fields, time, exhausted revisions or unavailable storage.
    pub fn save_local_public_hive_profile(
        &self,
        profile: &PublicHiveProfile,
        now: i64,
    ) -> Result<LocalPublicHiveProfile, TaskStoreError> {
        if now < 0 || !profile.is_valid() {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let prior = read_profile(&transaction)?;
        if prior.profile == *profile {
            return Ok(prior);
        }
        advance_local_profile_revision(&transaction)?;
        transaction.execute(
            "UPDATE hives SET name = ?1, updated_at = ?2
             WHERE id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)",
            params![profile.hive_name, now],
        )?;
        transaction.execute(
            "UPDATE operators SET display_name = ?1, updated_at = ?2
             WHERE id = (SELECT h.operator_id FROM hives h JOIN local_hive_identity l ON l.hive_id = h.id WHERE l.singleton = 1)",
            params![profile.operator_display_name, now],
        )?;
        transaction.execute(
            "UPDATE local_public_hive_profile SET contact_email = ?1 WHERE singleton = 1",
            params![profile.contact_email],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        let saved = read_profile(&transaction)?;
        transaction.commit()?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_replay_preserves_saved_profile_and_revision() {
        let store = TaskStore::in_memory().unwrap();
        let mut profile = store.local_public_hive_profile().unwrap().profile;
        profile.operator_display_name = "Bea".into();
        profile.contact_email = Some("bea@example.test".into());
        let saved = store.save_local_public_hive_profile(&profile, 10).unwrap();
        {
            let mut connection = store.connection().unwrap();
            let transaction = connection.transaction().unwrap();
            migrate(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(store.local_public_hive_profile().unwrap(), saved);
    }

    #[test]
    fn public_profile_save_is_owned_revisioned_and_idempotent() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let original = store.local_public_hive_profile().unwrap();
        assert_eq!(original.revision, 1);
        assert_eq!(original.profile.contact_email, None);
        let profile = PublicHiveProfile {
            hive_name: "Clover House".into(),
            operator_display_name: "Vicky Bee".into(),
            contact_email: Some("vicky@example.test".into()),
        };
        let saved = store.save_local_public_hive_profile(&profile, 10).unwrap();
        assert_eq!(saved.revision, 2);
        assert_eq!(
            store.save_local_public_hive_profile(&profile, 11).unwrap(),
            saved
        );
        let after = store.local_hive_identity().unwrap();
        assert_eq!(after.hive.id, identity.hive.id);
        assert_eq!(after.operator.id, identity.operator.id);
        assert_eq!(after.hive.apiary_id, identity.hive.apiary_id);
        assert_eq!(after.operator.display_name, "Vicky Bee");
        store.rename_local_hive("Evening Clover", 12).unwrap();
        let renamed = store.local_public_hive_profile().unwrap();
        assert_eq!(renamed.revision, 3);
        assert_eq!(renamed.profile.contact_email, profile.contact_email);
        store.rename_local_hive("Evening Clover", 13).unwrap();
        assert_eq!(store.local_public_hive_profile().unwrap(), renamed);
    }

    #[test]
    fn invalid_profile_cannot_partially_change_local_identity() {
        let store = TaskStore::in_memory().unwrap();
        let original = store.local_public_hive_profile().unwrap();
        let mut invalid = original.profile.clone();
        invalid.hive_name = "New name".into();
        invalid.contact_email = Some("bad contact".into());
        assert!(store.save_local_public_hive_profile(&invalid, 10).is_err());
        assert_eq!(store.local_public_hive_profile().unwrap(), original);
    }

    #[test]
    fn exhausted_revision_rolls_back_legacy_rename_and_profile_save() {
        let store = TaskStore::in_memory().unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE local_public_hive_profile SET revision = 9223372036854775807",
                [],
            )
            .unwrap();
        let original = store.local_public_hive_profile().unwrap();
        assert!(store.rename_local_hive("New name", 10).is_err());
        let mut changed = original.profile.clone();
        changed.operator_display_name = "Bea".into();
        assert!(store.save_local_public_hive_profile(&changed, 10).is_err());
        assert_eq!(store.local_public_hive_profile().unwrap(), original);
    }
}
