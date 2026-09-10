//! Public display identities, never membership or execution authority.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{ApiaryId, FederationNodeId, HiveId, LocalApiaryRole, OperatorId};

pub const FEDERATION_DIRECTORY_SCHEMA_VERSION: u16 = 1;
pub const MAX_APIARY_DIRECTORY_ENTRIES: usize = 256;
pub const MAX_PUBLIC_PROFILE_NAME_BYTES: usize = 120;
pub const MAX_PUBLIC_CONTACT_EMAIL_BYTES: usize = 254;
pub const MAX_DIRECTORY_SNAPSHOT_LIFETIME_SECONDS: i64 = 300;
pub const MAX_DIRECTORY_CLOCK_SKEW_SECONDS: i64 = 300;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublicHiveProfile {
    pub hive_name: String,
    pub operator_display_name: String,
    /// Operator-provided contact information, not a verified login identity.
    pub contact_email: Option<String>,
}

impl PublicHiveProfile {
    /// Validates canonical display fields. Callers may trim operator input before
    /// creating a profile, but signed payloads must not be silently normalized.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        valid_name(&self.hive_name)
            && valid_name(&self.operator_display_name)
            && self
                .contact_email
                .as_deref()
                .is_none_or(valid_contact_email)
    }

    /// Only the explicit join flow calls this, never ordinary profile saving.
    /// A custom name and an unknown operator remain unchanged.
    #[must_use]
    pub fn with_default_join_name(mut self) -> Self {
        if self.hive_name == "My Hive"
            && valid_name(&self.operator_display_name)
            && self.operator_display_name != "Operator"
            && let Some(first) = self.operator_display_name.split_whitespace().next()
        {
            let name = format!("{first}'s Hive");
            if valid_name(&name) {
                self.hive_name = name;
            }
        }
        self
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && value.len() <= MAX_PUBLIC_PROFILE_NAME_BYTES
        && !value.chars().any(char::is_control)
}

fn valid_contact_email(value: &str) -> bool {
    if value.len() > MAX_PUBLIC_CONTACT_EMAIL_BYTES
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty() && !domain.is_empty() && !domain.contains('@')
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublicHiveIdentity {
    pub node_id: FederationNodeId,
    pub hive_id: HiveId,
    pub operator_id: OperatorId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationProfileUpdatePayload {
    pub schema_version: u16,
    pub apiary_id: ApiaryId,
    pub identity: PublicHiveIdentity,
    pub revision: u64,
    pub profile: PublicHiveProfile,
}

impl FederationProfileUpdatePayload {
    /// Structural scope validation only. Persistence must also verify the pinned
    /// signature, active credential, and monotonic revision in its transaction.
    #[must_use]
    pub fn matches_member(&self, apiary_id: ApiaryId, member: PublicHiveIdentity) -> bool {
        self.schema_version == FEDERATION_DIRECTORY_SCHEMA_VERSION
            && self.apiary_id == apiary_id
            && self.identity == member
            && valid_revision(self.revision)
            && self.profile.is_valid()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationProfileUpdate {
    pub payload: FederationProfileUpdatePayload,
    pub signature: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryDirectoryEntry {
    pub identity: PublicHiveIdentity,
    pub profile: PublicHiveProfile,
    pub profile_revision: u64,
    pub role: LocalApiaryRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationDirectoryPayload {
    pub schema_version: u16,
    pub apiary_id: ApiaryId,
    pub keeper: PublicHiveIdentity,
    pub recipient: PublicHiveIdentity,
    pub revision: u64,
    pub entries: Vec<ApiaryDirectoryEntry>,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl FederationDirectoryPayload {
    /// Checks a complete directory's bounds and exact expected identities.
    /// Signature verification and rollback protection are separate mandatory
    /// persistence gates; passing this check grants no authority.
    #[must_use]
    pub fn matches_scope(
        &self,
        apiary_id: ApiaryId,
        keeper: PublicHiveIdentity,
        recipient: PublicHiveIdentity,
        now: i64,
    ) -> bool {
        if self.schema_version != FEDERATION_DIRECTORY_SCHEMA_VERSION
            || self.apiary_id != apiary_id
            || self.keeper != keeper
            || self.recipient != recipient
            || !valid_revision(self.revision)
            || self.entries.len() < 2
            || self.entries.len() > MAX_APIARY_DIRECTORY_ENTRIES
            || now < 0
            || self.issued_at < 0
            || self.issued_at > now.saturating_add(MAX_DIRECTORY_CLOCK_SKEW_SECONDS)
            || self.expires_at <= now
            || self.expires_at <= self.issued_at
            || self.expires_at.saturating_sub(self.issued_at)
                > MAX_DIRECTORY_SNAPSHOT_LIFETIME_SECONDS
            || keeper == recipient
        {
            return false;
        }
        let mut nodes = HashSet::new();
        let mut hives = HashSet::new();
        let mut operators = HashSet::new();
        let mut has_keeper = false;
        let mut has_recipient = false;
        for entry in &self.entries {
            if !entry.profile.is_valid()
                || !valid_revision(entry.profile_revision)
                || !nodes.insert(entry.identity.node_id)
                || !hives.insert(entry.identity.hive_id)
                || !operators.insert(entry.identity.operator_id)
            {
                return false;
            }
            if entry.role == LocalApiaryRole::Keeper {
                if entry.identity != keeper {
                    return false;
                }
                has_keeper = true;
            } else if entry.identity == keeper {
                return false;
            }
            if entry.identity == recipient {
                has_recipient = true;
            }
        }
        has_keeper && has_recipient
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationDirectorySnapshot {
    pub payload: FederationDirectoryPayload,
    pub signature: String,
}

fn valid_revision(revision: u64) -> bool {
    revision > 0 && i64::try_from(revision).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> PublicHiveProfile {
        PublicHiveProfile {
            hive_name: "My Hive".into(),
            operator_display_name: "Vicky Bee".into(),
            contact_email: Some("vicky@example.test".into()),
        }
    }
    fn identity() -> PublicHiveIdentity {
        PublicHiveIdentity {
            node_id: FederationNodeId::new(),
            hive_id: HiveId::new(),
            operator_id: OperatorId::new(),
        }
    }
    fn directory() -> FederationDirectoryPayload {
        let keeper = identity();
        let recipient = identity();
        FederationDirectoryPayload {
            schema_version: 1,
            apiary_id: ApiaryId::new(),
            keeper,
            recipient,
            revision: 1,
            issued_at: 100,
            expires_at: 400,
            entries: vec![
                ApiaryDirectoryEntry {
                    identity: keeper,
                    profile: profile(),
                    profile_revision: 1,
                    role: LocalApiaryRole::Keeper,
                },
                ApiaryDirectoryEntry {
                    identity: recipient,
                    profile: profile(),
                    profile_revision: 1,
                    role: LocalApiaryRole::Member,
                },
            ],
        }
    }
    fn valid(payload: &FederationDirectoryPayload) -> bool {
        payload.matches_scope(payload.apiary_id, payload.keeper, payload.recipient, 101)
    }

    #[test]
    fn join_names_preserve_custom_and_unknown_names() {
        assert_eq!(profile().with_default_join_name().hive_name, "Vicky's Hive");
        let mut custom = profile();
        custom.hive_name = "Clover House".into();
        assert_eq!(custom.clone().with_default_join_name(), custom);
        let mut unknown = profile();
        unknown.operator_display_name = "Operator".into();
        assert_eq!(unknown.clone().with_default_join_name(), unknown);
    }

    #[test]
    fn public_profile_bounds_reject_invalid_signed_fields() {
        assert!(profile().is_valid());
        for email in ["", "@host", "name@", "a@b@c", "a b@c", "a\nb@c"] {
            let mut candidate = profile();
            candidate.contact_email = Some(email.into());
            assert!(!candidate.is_valid(), "{email:?}");
        }
        for name in [
            String::new(),
            " Vicky".into(),
            "Vicky\nBee".into(),
            "x".repeat(121),
        ] {
            let mut candidate = profile();
            candidate.operator_display_name = name;
            assert!(!candidate.is_valid());
        }
        let mut no_email = profile();
        no_email.contact_email = None;
        assert!(no_email.is_valid());
    }

    #[test]
    fn complete_directory_requires_exact_scope_and_one_keeper() {
        let payload = directory();
        assert!(valid(&payload));
        assert!(!payload.matches_scope(ApiaryId::new(), payload.keeper, payload.recipient, 101));
        assert!(!payload.matches_scope(payload.apiary_id, identity(), payload.recipient, 101));
        assert!(!payload.matches_scope(payload.apiary_id, payload.keeper, identity(), 101));
        let mut candidate = payload.clone();
        candidate.entries[1].role = LocalApiaryRole::Keeper;
        assert!(!valid(&candidate));
        let mut candidate = payload.clone();
        candidate.entries[0].role = LocalApiaryRole::Member;
        assert!(!valid(&candidate));
        let mut candidate = payload;
        candidate.entries.pop();
        assert!(!valid(&candidate));
    }

    #[test]
    fn directory_rejects_duplicates_bad_revisions_and_expiry() {
        let payload = directory();
        for dimension in 0..3 {
            let mut candidate = payload.clone();
            match dimension {
                0 => candidate.entries[1].identity.node_id = candidate.keeper.node_id,
                1 => candidate.entries[1].identity.hive_id = candidate.keeper.hive_id,
                _ => candidate.entries[1].identity.operator_id = candidate.keeper.operator_id,
            }
            assert!(!valid(&candidate));
        }
        for revision in [0, u64::MAX] {
            let mut candidate = payload.clone();
            candidate.revision = revision;
            assert!(!valid(&candidate));
        }
        assert!(payload.matches_scope(payload.apiary_id, payload.keeper, payload.recipient, 99));
        let mut future = payload.clone();
        future.issued_at = 402;
        future.expires_at = 702;
        assert!(!future.matches_scope(payload.apiary_id, payload.keeper, payload.recipient, 101));
        assert!(!payload.matches_scope(payload.apiary_id, payload.keeper, payload.recipient, 400));
        let mut oversized = payload.clone();
        oversized.entries.resize(257, payload.entries[0].clone());
        assert!(!valid(&oversized));
    }

    #[test]
    fn profile_update_is_bound_to_the_exact_active_member() {
        let member = identity();
        let apiary = ApiaryId::new();
        let update = FederationProfileUpdatePayload {
            schema_version: 1,
            apiary_id: apiary,
            identity: member,
            revision: 1,
            profile: profile(),
        };
        assert!(update.matches_member(apiary, member));
        assert!(!update.matches_member(ApiaryId::new(), member));
        assert!(!update.matches_member(apiary, identity()));
    }
}
