//! What an Apiary policy revision actually contains.
//!
//! Policy was a bare integer: members accepted "revision 3" and revision 3 had
//! no body. This gives a revision SETTINGS — shared defaults a member converges
//! to — and makes the gap between the Apiary default and what a member actually
//! runs computable rather than assertable.

use serde::{Deserialize, Serialize};

use crate::{ApiaryId, FederationNodeId, HiveId, OperatorId};

pub const APIARY_POLICY_SCHEMA_VERSION: u16 = 1;

/// Bounded like every other federation manifest here.
pub const MAX_POLICY_SETTINGS: usize = 128;

const MAX_KEY_BYTES: usize = 120;
const MAX_VALUE_BYTES: usize = 2048;

/// ⚠️ SETTINGS THAT WOULD MAKE THIS REMOTE CONTROL RATHER THAN SHARED DEFAULTS.
///
/// Doc 98 excludes credentials, filesystem roots and provider permissions from
/// Keeper's reach, and the 2026-09-21 widening of Keeper VISIBILITY explicitly
/// did NOT supersede that. Watching a Hive is one thing; Keeper writing its
/// secrets or rewriting what its workers may execute is another, and nothing in
/// the interview asked for it.
///
/// Refused here rather than in a reviewer's head, because the tempting next
/// setting is always one of these and the prose exclusion is easy to not read.
const FORBIDDEN_KEY_PREFIXES: [&str; 6] = [
    "credential",
    "secret",
    "token",
    "workspace.root",
    "filesystem",
    "provider.permission",
];

/// One shared default, as a comparable key and value.
///
/// ⚠️ KEY/VALUE RATHER THAN PROSE, AND THE REASON OUTLIVES THE FORM: settings
/// are comparable, so DRIFT IS COMPUTABLE. A policy document would have to be
/// diffed as text to answer "has this member applied it", which in practice
/// means nobody answers it and the whole thing decays back into the
/// acknowledged-statement model this replaces.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryPolicySetting {
    pub key: String,
    pub value: String,
}

impl ApiaryPolicySetting {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let key = self.key.trim();
        !key.is_empty()
            && key == self.key
            && key.len() <= MAX_KEY_BYTES
            && self.value.len() <= MAX_VALUE_BYTES
            && key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
            && !Self::is_forbidden(key)
    }

    /// Whether this key reaches for something policy must never carry.
    #[must_use]
    pub fn is_forbidden(key: &str) -> bool {
        let lowered = key.to_ascii_lowercase();
        FORBIDDEN_KEY_PREFIXES
            .iter()
            .any(|forbidden| lowered.starts_with(forbidden))
    }
}

/// The Apiary's shared defaults for one revision, signed by Keeper for one
/// authenticated member node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryPolicySnapshotPayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub apiary_id: ApiaryId,
    /// The revision these settings ARE. The catalog already advertises the
    /// Apiary's current policy revision, so a member can tell whether the body
    /// it holds matches the revision it was told about.
    pub policy_revision: u64,
    pub settings_digest: String,
    pub settings: Vec<ApiaryPolicySetting>,
    pub keeper_node_id: FederationNodeId,
    pub keeper_hive_id: HiveId,
    pub keeper_operator_id: OperatorId,
    pub member_node_id: FederationNodeId,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl ApiaryPolicySnapshotPayload {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.policy_revision > 0
            && self.issued_at >= 0
            && self.expires_at > self.issued_at
            && self.settings.len() <= MAX_POLICY_SETTINGS
            && self.settings.iter().all(ApiaryPolicySetting::is_valid)
            && !self.has_duplicate_keys()
    }

    fn has_duplicate_keys(&self) -> bool {
        let mut keys: Vec<&str> = self.settings.iter().map(|s| s.key.as_str()).collect();
        keys.sort_unstable();
        let total = keys.len();
        keys.dedup();
        keys.len() != total
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryPolicySnapshot {
    pub payload: ApiaryPolicySnapshotPayload,
    pub signature: String,
}

/// One setting where a member differs from its Apiary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyDrift {
    pub key: String,
    /// What the Apiary says.
    pub expected: String,
    /// What this member actually runs. `None` means the member has not applied
    /// this setting at all, which is a DIFFERENT thing from overriding it and
    /// reads differently to whoever has to act.
    pub local: Option<String>,
}

/// How a member differs from the Apiary defaults it has been handed.
///
/// ⚠️ DRIFT IS REPORTED, NEVER PREVENTED, and that is the whole design. A
/// federation that cannot be deviated from is remote control; one that cannot
/// detect deviation is decoration. A local override still wins on the member's
/// own machine — it simply stops being invisible.
#[must_use]
pub fn policy_drift(
    expected: &[ApiaryPolicySetting],
    applied: &[ApiaryPolicySetting],
) -> Vec<PolicyDrift> {
    let mut drift = Vec::new();
    for setting in expected {
        let local = applied
            .iter()
            .find(|candidate| candidate.key == setting.key)
            .map(|candidate| candidate.value.clone());
        if local.as_deref() != Some(setting.value.as_str()) {
            drift.push(PolicyDrift {
                key: setting.key.clone(),
                expected: setting.value.clone(),
                local,
            });
        }
    }
    drift.sort_by(|first, second| first.key.cmp(&second.key));
    drift
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

    /// ⚠️ THE EXCLUSION DOC 98 KEEPS, REFUSED IN CODE. The 2026-09-21 decision
    /// widened Keeper VISIBILITY and deliberately did not touch this: watching a
    /// Hive is not the same as writing its secrets or its execution permissions.
    #[test]
    fn policy_cannot_carry_credentials_roots_or_provider_permissions() {
        for forbidden in [
            "credentials.jira",
            "credential",
            "secret.webhook",
            "token.github",
            "workspace.root",
            "filesystem.allow",
            "provider.permissions",
            "PROVIDER.PERMISSION.bash",
        ] {
            assert!(
                !setting(forbidden, "anything").is_valid(),
                "{forbidden:?} must never be distributable as policy"
            );
        }
    }

    #[test]
    fn ordinary_shared_defaults_are_accepted() {
        for allowed in [
            "checks.required",
            "checks.max_warnings",
            "workflow.review_required",
            "conventions.commit-style",
        ] {
            assert!(
                setting(allowed, "true").is_valid(),
                "{allowed:?} is a shared default"
            );
        }
        assert!(!setting("", "x").is_valid());
        assert!(!setting(" spaced", "x").is_valid());
        assert!(!setting("has space", "x").is_valid());
    }

    /// Not applied and overridden are different, and a reader has to act
    /// differently on each: one member never converged, the other decided not to.
    #[test]
    fn drift_distinguishes_never_applied_from_overridden() {
        let expected = vec![
            setting("checks.required", "true"),
            setting("checks.max_warnings", "0"),
            setting("workflow.review_required", "true"),
        ];
        let applied = vec![
            setting("checks.required", "true"),
            setting("checks.max_warnings", "5"),
        ];
        let drift = policy_drift(&expected, &applied);
        assert_eq!(drift.len(), 2, "one override and one never applied");
        assert_eq!(
            drift[0],
            PolicyDrift {
                key: "checks.max_warnings".to_owned(),
                expected: "0".to_owned(),
                local: Some("5".to_owned()),
            },
            "an override reports what the member actually runs"
        );
        assert_eq!(
            drift[1],
            PolicyDrift {
                key: "workflow.review_required".to_owned(),
                expected: "true".to_owned(),
                local: None,
            },
            "never applied is None, not an empty string"
        );
    }

    /// ⚠️ A MEMBER'S EXTRA LOCAL SETTINGS ARE NOT DRIFT. Policy describes shared
    /// defaults, not the whole of a member's configuration. Reporting a
    /// member's own unrelated settings as deviation would make every Hive look
    /// non-compliant for having a life of its own.
    #[test]
    fn local_settings_the_apiary_does_not_mention_are_not_drift() {
        let expected = vec![setting("checks.required", "true")];
        let applied = vec![
            setting("checks.required", "true"),
            setting("local.editor", "vim"),
        ];
        assert!(policy_drift(&expected, &applied).is_empty());
    }

    #[test]
    fn a_converged_member_has_no_drift_and_an_empty_policy_demands_nothing() {
        let expected = vec![setting("checks.required", "true")];
        assert!(policy_drift(&expected, &expected).is_empty());
        assert!(policy_drift(&[], &[setting("local.thing", "1")]).is_empty());
    }

    #[test]
    fn a_snapshot_refuses_duplicate_keys_and_oversized_manifests() {
        let base = ApiaryPolicySnapshotPayload {
            schema_version: APIARY_POLICY_SCHEMA_VERSION,
            protocol_version: crate::FEDERATION_PROTOCOL_VERSION,
            apiary_id: ApiaryId::new(),
            policy_revision: 3,
            settings_digest: "digest".to_owned(),
            settings: vec![setting("checks.required", "true")],
            keeper_node_id: FederationNodeId::new(),
            keeper_hive_id: HiveId::new(),
            keeper_operator_id: OperatorId::new(),
            member_node_id: FederationNodeId::new(),
            issued_at: 1_790_000_000,
            expires_at: 1_790_000_300,
        };
        assert!(base.is_valid());

        let mut duplicated = base.clone();
        duplicated
            .settings
            .push(setting("checks.required", "false"));
        assert!(
            !duplicated.is_valid(),
            "two values for one key have no defined meaning and must be refused"
        );

        let mut zero_revision = base.clone();
        zero_revision.policy_revision = 0;
        assert!(!zero_revision.is_valid());

        let mut oversized = base;
        oversized.settings = (0..=MAX_POLICY_SETTINGS)
            .map(|index| setting(&format!("checks.item-{index}"), "1"))
            .collect();
        assert!(!oversized.is_valid());
    }
}
