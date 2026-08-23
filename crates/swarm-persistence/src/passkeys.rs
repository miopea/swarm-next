//! Passkeys registered for signing in to this Hive.
//!
//! Stored opaquely: the credential is whatever the `WebAuthn` library needs to
//! verify a signature later, and persistence does not interpret it. What this
//! layer does own is which domain a credential belongs to and what the operator
//! calls it — the two facts that decide whether it can be offered and whether
//! it can be recognised before being removed.

use rusqlite::{OptionalExtension, params};

use super::{TaskStore, TaskStoreError};

/// One registered passkey, as the control room needs to show it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredPasskey {
    pub credential_id: String,
    pub relying_party: String,
    pub label: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

impl TaskStore {
    /// Records a passkey the operator just registered.
    ///
    /// # Errors
    /// Rejects an empty label and returns a persistence error otherwise.
    pub fn add_operator_passkey(
        &self,
        credential_id: &str,
        relying_party: &str,
        label: &str,
        credential: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 120 {
            return Err(TaskStoreError::IntegrityFailure(
                "a passkey needs a name you will recognise later".into(),
            ));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO operator_passkeys
                 (credential_id, relying_party, label, credential, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(credential_id) DO UPDATE SET
                 relying_party = excluded.relying_party,
                 label = excluded.label,
                 credential = excluded.credential",
            params![credential_id, relying_party, label, credential, now],
        )?;
        Ok(())
    }

    /// Passkeys usable at this domain.
    ///
    /// Filtered by relying party because a credential registered elsewhere
    /// cannot authenticate here — offering it would produce a browser error
    /// rather than a sign-in.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn operator_passkeys_for(
        &self,
        relying_party: &str,
    ) -> Result<Vec<(RegisteredPasskey, String)>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT credential_id, relying_party, label, created_at, last_used_at, credential
             FROM operator_passkeys WHERE relying_party = ?1
             ORDER BY created_at",
        )?;
        let rows = statement.query_map([relying_party], |row| {
            Ok((
                RegisteredPasskey {
                    credential_id: row.get(0)?,
                    relying_party: row.get(1)?,
                    label: row.get(2)?,
                    created_at: row.get(3)?,
                    last_used_at: row.get(4)?,
                },
                row.get::<_, String>(5)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every registered passkey, for the list the operator manages.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn operator_passkeys(&self) -> Result<Vec<RegisteredPasskey>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT credential_id, relying_party, label, created_at, last_used_at
             FROM operator_passkeys ORDER BY created_at",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(RegisteredPasskey {
                credential_id: row.get(0)?,
                relying_party: row.get(1)?,
                label: row.get(2)?,
                created_at: row.get(3)?,
                last_used_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Replaces a credential after a successful sign-in, and records when.
    ///
    /// The stored credential carries a signature counter that moves with each
    /// use, so writing it back is what keeps a cloned authenticator detectable.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_passkey_use(
        &self,
        credential_id: &str,
        credential: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE operator_passkeys SET credential = ?2, last_used_at = ?3
             WHERE credential_id = ?1",
            params![credential_id, credential, now],
        )?;
        Ok(())
    }

    /// Removes a passkey, which is the only way to revoke one.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn remove_operator_passkey(&self, credential_id: &str) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "DELETE FROM operator_passkeys WHERE credential_id = ?1",
            [credential_id],
        )? > 0)
    }

    /// Whether any passkey exists at all, so the sign-in panel knows whether to
    /// offer one.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn has_operator_passkey(&self, relying_party: &str) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT 1 FROM operator_passkeys WHERE relying_party = ?1 LIMIT 1",
                [relying_party],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
