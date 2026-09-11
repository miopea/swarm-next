//! Preserve departed membership receipts while allowing a new approved epoch.

pub(crate) fn migrate(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE apiary_federation_memberships_v168 (
            receipt_id TEXT PRIMARY KEY,
            invitation_id TEXT NOT NULL UNIQUE REFERENCES apiary_federation_invitations(id),
            apiary_id TEXT NOT NULL REFERENCES apiaries(id),
            member_node_id TEXT NOT NULL,
            member_hive_id TEXT NOT NULL REFERENCES hives(id),
            member_operator_id TEXT NOT NULL REFERENCES operators(id),
            receipt_json TEXT NOT NULL,
            node_credential BLOB NOT NULL CHECK (length(node_credential) = 32),
            credential_digest BLOB NOT NULL UNIQUE CHECK (length(credential_digest) = 32),
            joined_at INTEGER NOT NULL CHECK (joined_at >= 0),
            credential_expires_at INTEGER NOT NULL CHECK (credential_expires_at > joined_at),
            state TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active','departed')),
            departed_at INTEGER CHECK (departed_at >= joined_at)
        );
        INSERT INTO apiary_federation_memberships_v168
            SELECT * FROM apiary_federation_memberships;
        DROP TABLE apiary_federation_memberships;
        ALTER TABLE apiary_federation_memberships_v168 RENAME TO apiary_federation_memberships;
        CREATE UNIQUE INDEX active_federation_hive ON apiary_federation_memberships(member_hive_id) WHERE state = 'active';
        CREATE UNIQUE INDEX active_federation_operator ON apiary_federation_memberships(member_operator_id) WHERE state = 'active';
        CREATE UNIQUE INDEX active_federation_node ON apiary_federation_memberships(apiary_id, member_node_id) WHERE state = 'active';
        CREATE TRIGGER immutable_apiary_federation_membership
            BEFORE UPDATE OF receipt_id, invitation_id, apiary_id,
                member_node_id, member_hive_id, member_operator_id, receipt_json, joined_at
            ON apiary_federation_memberships
            BEGIN SELECT RAISE(ABORT, 'Federation membership identity is immutable'); END;"
    )?;
    tx.pragma_update(
        None,
        "user_version",
        super::FEDERATION_MEMBERSHIP_EPOCHS_SCHEMA_VERSION,
    )
}
