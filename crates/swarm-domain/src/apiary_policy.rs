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

/// How a member differs from the settings it has been handed.
///
/// ⚠️ SUPERSEDED IN MEANING BY ADR 0113. This was written when Apiary settings
/// were defaults a member could override, so drift meant "chose differently".
/// The operator ruled on 2026-10-12 that every Apiary setting is REQUIRED, so
/// drift now means "not in effect yet" and is a fault to fix — see
/// [`policy_gaps`], which says why and what fixes it.
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

/// A setting an Apiary can require, which a member knows how to apply.
///
/// ⚠️ ONLY THESE, AND THAT IS THE POINT. Under "always required" (ADR 0113) a
/// setting no member acts on would read as a requirement and change nothing —
/// the inert `checks.max_warnings` this replaces is exactly that. A Keeper can
/// set only what members enforce; each new one arrives with its enforcement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequiredSetting {
    /// Whether ordinary releases install themselves while the operator is away
    /// (ADR 0110). Never a remote restart: it changes WHETHER a member installs
    /// on its own, not WHEN, and protocol changes still wait for its operator.
    ReleaseAutoInstall,
}

impl RequiredSetting {
    pub const ALL: [Self; 1] = [Self::ReleaseAutoInstall];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ReleaseAutoInstall => "release.auto_install",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.key() == key)
    }

    /// The values a Keeper may require.
    #[must_use]
    pub const fn values(self) -> &'static [&'static str] {
        match self {
            Self::ReleaseAutoInstall => &["on", "off"],
        }
    }
}

/// Checks that a Keeper is requiring only what members can apply.
///
/// # Errors
/// Names the first setting no member knows, or a value it does not accept.
pub fn validate_required_settings(settings: &[ApiaryPolicySetting]) -> Result<(), String> {
    for setting in settings {
        let Some(known) = RequiredSetting::from_key(&setting.key) else {
            return Err(format!("{} is not a setting Hives can apply", setting.key));
        };
        if !known.values().contains(&setting.value.as_str()) {
            return Err(format!(
                "{} must be one of {}",
                setting.key,
                known.values().join(", ")
            ));
        }
    }
    Ok(())
}

/// What an Apiary requires of an on/off setting, if it requires anything.
#[must_use]
pub fn required_switch(settings: &[ApiaryPolicySetting], setting: RequiredSetting) -> Option<bool> {
    settings
        .iter()
        .find(|candidate| candidate.key == setting.key())
        .and_then(|candidate| match candidate.value.as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        })
}

/// Why one required setting is not in effect on one Hive, which is also what
/// fixes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum PolicyGapReason {
    /// The Hive has not reported to this Keeper at all.
    NotReported,
    /// It has not picked up this revision of the policy yet.
    NotReceived,
    /// It holds this policy but its release does not apply this setting.
    ReleaseCannotApply,
    /// It applied a different value.
    Different { local: String },
}

/// One required setting not in effect on one Hive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyGap {
    pub key: String,
    pub expected: String,
    #[serde(flatten)]
    pub reason: PolicyGapReason,
}

/// What a member last told its Keeper about its policy.
#[derive(Clone, Copy, Debug)]
pub struct ReportedPolicy<'report> {
    /// The policy revision it holds a body for.
    pub revision: Option<u64>,
    /// What it applied.
    pub applied: &'report [ApiaryPolicySetting],
}

/// The required settings not in effect on one Hive, each with why.
///
/// ⚠️ EMPTY FOR A COMPLIANT HIVE, AND THAT IS THE WHOLE OUTPUT FOR ONE. The
/// operator's rule for everything a Keeper sees: "we don't want to generate too
/// much random noise for the sake of data that we wouldn't act upon." A Hive in
/// policy produces nothing to read.
#[must_use]
pub fn policy_gaps(
    required: &[ApiaryPolicySetting],
    current_revision: u64,
    reported: Option<ReportedPolicy<'_>>,
) -> Vec<PolicyGap> {
    let mut gaps = Vec::new();
    for setting in required {
        let reason = match reported {
            None => PolicyGapReason::NotReported,
            Some(report) if report.revision.is_none_or(|held| held < current_revision) => {
                PolicyGapReason::NotReceived
            }
            Some(report) => match report
                .applied
                .iter()
                .find(|applied| applied.key == setting.key)
            {
                None => PolicyGapReason::ReleaseCannotApply,
                Some(applied) if applied.value == setting.value => continue,
                Some(applied) => PolicyGapReason::Different {
                    local: applied.value.clone(),
                },
            },
        };
        gaps.push(PolicyGap {
            key: setting.key.clone(),
            expected: setting.value.clone(),
            reason,
        });
    }
    gaps
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

    /// ⚠️ A KEEPER MAY REQUIRE ONLY WHAT MEMBERS ENFORCE. Under "always required"
    /// an inert key would read as a requirement and change nothing.
    #[test]
    fn a_keeper_can_require_only_settings_members_apply() {
        assert_eq!(
            validate_required_settings(&[setting("release.auto_install", "on")]),
            Ok(())
        );
        assert_eq!(
            validate_required_settings(&[setting("release.auto_install", "sometimes")]),
            Err("release.auto_install must be one of on, off".to_owned())
        );
        assert_eq!(
            validate_required_settings(&[setting("checks.max_warnings", "0")]),
            Err("checks.max_warnings is not a setting Hives can apply".to_owned())
        );
        let policy = [setting("release.auto_install", "off")];
        assert_eq!(
            required_switch(&policy, RequiredSetting::ReleaseAutoInstall),
            Some(false)
        );
        assert_eq!(
            required_switch(&[], RequiredSetting::ReleaseAutoInstall),
            None
        );
    }

    /// Each gap says what fixes it, and a compliant Hive says nothing at all.
    #[test]
    fn a_gap_names_why_and_a_compliant_hive_has_none() {
        let required = [setting("release.auto_install", "on")];
        let gap = |reported| {
            policy_gaps(&required, 4, reported)
                .into_iter()
                .map(|gap| gap.reason)
                .collect::<Vec<_>>()
        };
        assert_eq!(gap(None), vec![PolicyGapReason::NotReported]);
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: Some(3),
                applied: &[]
            })),
            vec![PolicyGapReason::NotReceived]
        );
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: None,
                applied: &[]
            })),
            vec![PolicyGapReason::NotReceived]
        );
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: Some(4),
                applied: &[]
            })),
            vec![PolicyGapReason::ReleaseCannotApply]
        );
        let other = [setting("release.auto_install", "off")];
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: Some(4),
                applied: &other
            })),
            vec![PolicyGapReason::Different {
                local: "off".to_owned()
            }]
        );
        assert!(
            gap(Some(ReportedPolicy {
                revision: Some(4),
                applied: &required
            }))
            .is_empty()
        );
        assert!(
            policy_gaps(&[], 4, None).is_empty(),
            "no requirements, nothing to say"
        );
    }
}
