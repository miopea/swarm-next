//! What an Apiary policy revision actually contains.
//!
//! Policy was a bare integer: members accepted "revision 3" and revision 3 had
//! no body. This gives a revision SETTINGS — shared defaults a member converges
//! to — and makes the gap between the Apiary default and what a member actually
//! runs computable rather than assertable.

use serde::{Deserialize, Serialize};

use crate::{
    ApiaryId, FederationNodeId, HiveId, NightWatchSchedule, OperatorId, QueenAutonomyLevel,
    QueenAutonomyPolicy, SwarmVersion,
};

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

/// A setting an Apiary can require, which is enforced somewhere real.
///
/// ⚠️ ONLY THESE, AND THAT IS THE POINT. Under "always required" (ADR 0113) a
/// setting nothing enforces would read as a requirement and change nothing —
/// the inert `checks.max_warnings` this replaces is exactly that. A Keeper can
/// set only what is enforced; each new one arrives with its enforcement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequiredSetting {
    /// Whether ordinary releases install themselves while the operator is away
    /// (ADR 0110). Never a remote restart: it changes WHETHER a member installs
    /// on its own, not WHEN, and protocol changes still wait for its operator.
    ReleaseAutoInstall,
    /// The oldest release that may take shared work. A Hive below it keeps its
    /// own work but cannot claim Apiary tasks or take a handoff until it updates.
    SharedWorkMinimumVersion,
    /// How far each member's Queen may carry work while its operator is at the
    /// Hive, away, or in Night Watch — one setting per presence tier, because
    /// that is how a Hive already holds it.
    QueenAutonomyAtHive,
    QueenAutonomyAway,
    QueenAutonomyNightWatch,
    /// The daily hours each member runs Night Watch, in that member's own time
    /// zone: "22:00-07:00" is ten at night to seven in the morning wherever the
    /// Hive is. The time zone stays the member's, because its operator's night
    /// is local to them.
    NightWatchHours,
}

/// What kind of value a required setting takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequiredValue {
    /// One of a fixed set.
    Choice(&'static [&'static str]),
    /// A release number, such as `1.16.9`.
    Release,
    /// A daily window, such as `22:00-07:00`.
    DailyHours,
}

/// Where a required setting is made true.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyEnforcer {
    /// The member applies it to itself and reports it as applied.
    Member,
    /// The Keeper enforces it at its own door. Needed when the Hive that falls
    /// short is by definition one that cannot be trusted to know the rule — an
    /// older release does not know a minimum version exists.
    Keeper,
}

impl RequiredSetting {
    pub const ALL: [Self; 6] = [
        Self::ReleaseAutoInstall,
        Self::SharedWorkMinimumVersion,
        Self::QueenAutonomyAtHive,
        Self::QueenAutonomyAway,
        Self::QueenAutonomyNightWatch,
        Self::NightWatchHours,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ReleaseAutoInstall => "release.auto_install",
            Self::SharedWorkMinimumVersion => "shared_work.minimum_version",
            Self::QueenAutonomyAtHive => "queen.autonomy.at_hive",
            Self::QueenAutonomyAway => "queen.autonomy.away",
            Self::QueenAutonomyNightWatch => "queen.autonomy.night_watch",
            Self::NightWatchHours => "night_watch.hours",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.key() == key)
    }

    /// What kind of value a Keeper sets.
    #[must_use]
    pub const fn value(self) -> RequiredValue {
        match self {
            Self::ReleaseAutoInstall => RequiredValue::Choice(&["on", "off"]),
            Self::SharedWorkMinimumVersion => RequiredValue::Release,
            Self::QueenAutonomyAtHive | Self::QueenAutonomyAway | Self::QueenAutonomyNightWatch => {
                RequiredValue::Choice(&["advisory", "coordinate", "local_execution"])
            }
            Self::NightWatchHours => RequiredValue::DailyHours,
        }
    }

    /// Whether `value` is one this setting accepts.
    #[must_use]
    pub fn accepts(self, value: &str) -> bool {
        match self.value() {
            RequiredValue::Choice(choices) => choices.contains(&value),
            // A release number. A development build is no one's minimum.
            RequiredValue::Release => {
                SwarmVersion::parse(value).is_some_and(|version| !version.is_development())
            }
            RequiredValue::DailyHours => parse_daily_hours(value).is_some(),
        }
    }

    /// The release from which a member applies this setting by itself unless
    /// its own operator still has to choose something, or `None` when a member
    /// that knows the setting always applies it.
    ///
    /// ⚠️ THIS IS HOW THE KEEPER TELLS "UPDATE IT" FROM "ITS OPERATOR HAS TO
    /// CHOOSE", from the release a member reports, with no new field in the
    /// report. Night Watch hours need the member's own time zone; a Hive that
    /// never chose one cannot apply them, and telling its operator to update
    /// would send them after the wrong fix. Keep this in step with the release
    /// that first carries the setting.
    #[must_use]
    pub const fn local_choice_since(self) -> Option<&'static str> {
        match self {
            Self::NightWatchHours => Some("1.17.2"),
            Self::ReleaseAutoInstall
            | Self::SharedWorkMinimumVersion
            | Self::QueenAutonomyAtHive
            | Self::QueenAutonomyAway
            | Self::QueenAutonomyNightWatch => None,
        }
    }

    #[must_use]
    pub const fn enforced_by(self) -> PolicyEnforcer {
        match self {
            Self::ReleaseAutoInstall
            | Self::QueenAutonomyAtHive
            | Self::QueenAutonomyAway
            | Self::QueenAutonomyNightWatch
            | Self::NightWatchHours => PolicyEnforcer::Member,
            Self::SharedWorkMinimumVersion => PolicyEnforcer::Keeper,
        }
    }
}

/// Checks that a Keeper is requiring only what is enforced, with values it
/// accepts.
///
/// # Errors
/// Names the first setting nothing enforces, or a value it does not accept.
pub fn validate_required_settings(settings: &[ApiaryPolicySetting]) -> Result<(), String> {
    for setting in settings {
        let Some(known) = RequiredSetting::from_key(&setting.key) else {
            return Err(format!("{} is not a setting Hives can apply", setting.key));
        };
        if !known.accepts(&setting.value) {
            return Err(match known.value() {
                RequiredValue::Choice(choices) => {
                    format!("{} must be one of {}", setting.key, choices.join(", "))
                }
                RequiredValue::Release => {
                    format!("{} must be a release version such as 1.16.9", setting.key)
                }
                RequiredValue::DailyHours => format!(
                    "{} must be two different times of day, such as 22:00-07:00",
                    setting.key
                ),
            });
        }
    }
    Ok(())
}

/// The oldest release an Apiary lets take shared work, if it sets one.
#[must_use]
pub fn minimum_shared_work_version(settings: &[ApiaryPolicySetting]) -> Option<SwarmVersion> {
    settings
        .iter()
        .find(|setting| setting.key == RequiredSetting::SharedWorkMinimumVersion.key())
        .and_then(|setting| SwarmVersion::parse(&setting.value))
}

/// Why a Hive running `reported` may not take shared work under `minimum`, or
/// `None` when it may. Said in words the Hive's operator can act on.
#[must_use]
pub fn shared_work_refusal(minimum: &SwarmVersion, reported: Option<&str>) -> Option<String> {
    match reported.and_then(SwarmVersion::parse) {
        Some(running) if running.at_least(minimum) => None,
        Some(running) => Some(format!(
            "this Hive runs {running}; the Apiary needs {minimum} or newer to take shared work, so update it"
        )),
        // Fails closed: a requirement nobody can check is not a requirement met.
        None => Some(format!(
            "this Hive has not told the Keeper which release it runs; the Apiary needs {minimum} or newer to take shared work"
        )),
    }
}

/// A daily window written `HH:MM-HH:MM`, such as `22:00-07:00`. Only the
/// zero-padded form is accepted, so one window has one spelling and a member's
/// applied value compares equal to the Keeper's.
#[must_use]
pub fn parse_daily_hours(value: &str) -> Option<NightWatchSchedule> {
    fn minute(time: &str) -> Option<u16> {
        let (hours, minutes) = time.split_once(':')?;
        if hours.len() != 2 || minutes.len() != 2 {
            return None;
        }
        let hours: u16 = hours.parse().ok()?;
        let minutes: u16 = minutes.parse().ok()?;
        (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
    }
    let (start, end) = value.split_once('-')?;
    NightWatchSchedule::new(minute(start)?, minute(end)?).ok()
}

/// The Night Watch hours an Apiary requires, if it requires any.
#[must_use]
pub fn required_night_watch_hours(settings: &[ApiaryPolicySetting]) -> Option<NightWatchSchedule> {
    settings
        .iter()
        .find(|setting| setting.key == RequiredSetting::NightWatchHours.key())
        .and_then(|setting| parse_daily_hours(&setting.value))
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

/// What an Apiary requires of Queen's autonomy, tier by tier. `None` leaves
/// that tier to the Hive's own operator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RequiredQueenAutonomy {
    pub at_hive: Option<QueenAutonomyLevel>,
    pub away: Option<QueenAutonomyLevel>,
    pub night_watch: Option<QueenAutonomyLevel>,
}

impl RequiredQueenAutonomy {
    #[must_use]
    pub fn from_settings(settings: &[ApiaryPolicySetting]) -> Self {
        let level = |setting: RequiredSetting| {
            settings
                .iter()
                .find(|candidate| candidate.key == setting.key())
                .and_then(|candidate| candidate.value.parse().ok())
        };
        Self {
            at_hive: level(RequiredSetting::QueenAutonomyAtHive),
            away: level(RequiredSetting::QueenAutonomyAway),
            night_watch: level(RequiredSetting::QueenAutonomyNightWatch),
        }
    }

    /// The policy in effect: the Apiary's level wherever it requires one, the
    /// Hive's own everywhere else.
    ///
    /// ⚠️ OVERRIDES WITHOUT ERASING (ADR 0113). The Hive keeps its own choice
    /// underneath, so lifting a requirement hands back what its operator chose
    /// rather than whatever the Apiary last said.
    #[must_use]
    pub fn apply(self, own: QueenAutonomyPolicy) -> QueenAutonomyPolicy {
        QueenAutonomyPolicy {
            at_hive: self.at_hive.unwrap_or(own.at_hive),
            away: self.away.unwrap_or(own.away),
            night_watch: self.night_watch.unwrap_or(own.night_watch),
        }
    }

    /// What to keep as the Hive's own choice when its operator asks for
    /// `requested`: their choice wherever it is theirs, and their previous one
    /// wherever the Apiary decides.
    ///
    /// # Errors
    /// Refuses, saying who can change it, a request to move a tier the Apiary
    /// requires. Asking for the required level is not a change and passes, so a
    /// screen that sends every tier back can still change the free ones.
    pub fn own_choice(
        self,
        requested: QueenAutonomyPolicy,
        previous_own: QueenAutonomyPolicy,
    ) -> Result<QueenAutonomyPolicy, String> {
        let tier = |required: Option<QueenAutonomyLevel>,
                    requested: QueenAutonomyLevel,
                    previous: QueenAutonomyLevel| match required {
            None => Ok(requested),
            Some(level) if level == requested => Ok(previous),
            Some(_) => Err(
                "your Apiary sets this Queen autonomy level; only its Keeper can change it"
                    .to_owned(),
            ),
        };
        Ok(QueenAutonomyPolicy {
            at_hive: tier(self.at_hive, requested.at_hive, previous_own.at_hive)?,
            away: tier(self.away, requested.away, previous_own.away)?,
            night_watch: tier(
                self.night_watch,
                requested.night_watch,
                previous_own.night_watch,
            )?,
        })
    }
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
    /// Its release applies this setting, but its own operator has to choose
    /// something first — for Night Watch hours, the Hive's time zone.
    NeedsLocalChoice,
    /// It applied a different value.
    Different { local: String },
    /// Its release is older than the Apiary's minimum, so the Keeper refuses it
    /// shared work.
    BelowMinimum { running: String },
}

/// One required setting not in effect on one Hive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyGap {
    pub key: String,
    pub expected: String,
    #[serde(flatten)]
    pub reason: PolicyGapReason,
}

/// What a member last told its Keeper about its policy and itself.
#[derive(Clone, Copy, Debug)]
pub struct ReportedPolicy<'report> {
    /// The policy revision it holds a body for.
    pub revision: Option<u64>,
    /// What it applied.
    pub applied: &'report [ApiaryPolicySetting],
    /// The release it runs.
    pub version: &'report str,
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
        // Enforced by the Keeper itself: nothing to receive or apply, only a
        // release to compare.
        if RequiredSetting::from_key(&setting.key)
            .is_some_and(|known| known.enforced_by() == PolicyEnforcer::Keeper)
        {
            let reason = match reported {
                None => PolicyGapReason::NotReported,
                Some(report) => match SwarmVersion::parse(&setting.value) {
                    Some(minimum)
                        if shared_work_refusal(&minimum, Some(report.version)).is_some() =>
                    {
                        PolicyGapReason::BelowMinimum {
                            running: report.version.to_owned(),
                        }
                    }
                    _ => continue,
                },
            };
            gaps.push(PolicyGap {
                key: setting.key.clone(),
                expected: setting.value.clone(),
                reason,
            });
            continue;
        }
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
                None => match RequiredSetting::from_key(&setting.key)
                    .and_then(RequiredSetting::local_choice_since)
                    .and_then(SwarmVersion::parse)
                {
                    Some(since)
                        if SwarmVersion::parse(report.version)
                            .is_some_and(|running| running.at_least(&since)) =>
                    {
                        PolicyGapReason::NeedsLocalChoice
                    }
                    _ => PolicyGapReason::ReleaseCannotApply,
                },
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
                applied: &[],
                version: "1.16.9",
            })),
            vec![PolicyGapReason::NotReceived]
        );
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: None,
                applied: &[],
                version: "1.16.9",
            })),
            vec![PolicyGapReason::NotReceived]
        );
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: Some(4),
                applied: &[],
                version: "1.16.9",
            })),
            vec![PolicyGapReason::ReleaseCannotApply]
        );
        let other = [setting("release.auto_install", "off")];
        assert_eq!(
            gap(Some(ReportedPolicy {
                revision: Some(4),
                applied: &other,
                version: "1.16.9",
            })),
            vec![PolicyGapReason::Different {
                local: "off".to_owned()
            }]
        );
        assert!(
            gap(Some(ReportedPolicy {
                revision: Some(4),
                applied: &required,
                version: "1.16.9",
            }))
            .is_empty()
        );
        assert!(
            policy_gaps(&[], 4, None).is_empty(),
            "no requirements, nothing to say"
        );
    }

    /// ⚠️ THE KEEPER ENFORCES A MINIMUM, BECAUSE THE HIVE BELOW IT CANNOT. An
    /// older release does not know a minimum exists, so the refusal and the
    /// roster line both come from what the Hive reported, not what it applied.
    #[test]
    fn a_minimum_version_refuses_older_releases_and_counts_dev_builds_by_number() {
        assert_eq!(
            validate_required_settings(&[setting("shared_work.minimum_version", "1.16.9")]),
            Ok(())
        );
        assert_eq!(
            validate_required_settings(&[setting(
                "shared_work.minimum_version",
                "1.16.9-dev-abc-1"
            )]),
            Err("shared_work.minimum_version must be a release version such as 1.16.9".to_owned()),
            "a development build is no one's minimum"
        );
        let minimum = SwarmVersion::parse("1.16.9").unwrap();
        assert_eq!(shared_work_refusal(&minimum, Some("1.16.9")), None);
        assert_eq!(shared_work_refusal(&minimum, Some("1.17.0")), None);
        assert_eq!(
            shared_work_refusal(&minimum, Some("1.16.9-dev-e46308f51643-20261012195505")),
            None,
            "a build of 1.16.9 from the checkout carries what 1.16.9 does"
        );
        assert_eq!(
            shared_work_refusal(&minimum, Some("1.16.8")),
            Some("this Hive runs 1.16.8; the Apiary needs 1.16.9 or newer to take shared work, so update it".to_owned())
        );
        assert!(
            shared_work_refusal(&minimum, None).is_some(),
            "unknown fails closed"
        );

        let required = [setting("shared_work.minimum_version", "1.16.9")];
        let reasons = |version: &str| {
            policy_gaps(
                &required,
                4,
                Some(ReportedPolicy {
                    revision: None,
                    applied: &[],
                    version,
                }),
            )
            .into_iter()
            .map(|gap| gap.reason)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            reasons("1.16.8"),
            vec![PolicyGapReason::BelowMinimum {
                running: "1.16.8".to_owned()
            }]
        );
        assert!(
            reasons("1.16.9").is_empty(),
            "nothing to receive or apply: met is met"
        );
        assert_eq!(
            RequiredSetting::SharedWorkMinimumVersion.enforced_by(),
            PolicyEnforcer::Keeper
        );
    }

    /// ⚠️ A REQUIRED TIER WINS AND THE HIVE'S OWN CHOICE SURVIVES UNDER IT.
    /// Lifting the requirement must hand back what the operator chose, not
    /// whatever the Apiary last said.
    #[test]
    fn a_required_queen_autonomy_tier_overrides_without_erasing() {
        use QueenAutonomyLevel::{Advisory, Coordinate, LocalExecution};
        let own = QueenAutonomyPolicy {
            at_hive: Coordinate,
            away: Coordinate,
            night_watch: LocalExecution,
        };
        assert_eq!(
            validate_required_settings(&[setting("queen.autonomy.night_watch", "advisory")]),
            Ok(())
        );
        assert_eq!(
            validate_required_settings(&[setting("queen.autonomy.away", "unlimited")]),
            Err(
                "queen.autonomy.away must be one of advisory, coordinate, local_execution"
                    .to_owned()
            )
        );
        let required = RequiredQueenAutonomy::from_settings(&[
            setting("queen.autonomy.night_watch", "advisory"),
            setting("release.auto_install", "on"),
        ]);
        assert_eq!(
            required.apply(own),
            QueenAutonomyPolicy {
                night_watch: Advisory,
                ..own
            },
            "only the required tier moves"
        );
        assert_eq!(
            RequiredQueenAutonomy::from_settings(&[]).apply(own),
            own,
            "lifted, the operator's own choice is back"
        );

        // The screen sends every tier back. A free tier changes; the locked one
        // is sent at its required level and keeps the operator's own underneath.
        let requested = QueenAutonomyPolicy {
            at_hive: Advisory,
            away: Coordinate,
            night_watch: Advisory,
        };
        assert_eq!(
            required.own_choice(requested, own),
            Ok(QueenAutonomyPolicy {
                at_hive: Advisory,
                ..own
            })
        );
        assert_eq!(
            required.own_choice(
                QueenAutonomyPolicy {
                    night_watch: LocalExecution,
                    ..requested
                },
                own
            ),
            Err(
                "your Apiary sets this Queen autonomy level; only its Keeper can change it"
                    .to_owned()
            ),
            "moving a locked tier is refused with who can change it"
        );
        assert_eq!(
            RequiredSetting::QueenAutonomyAway.enforced_by(),
            PolicyEnforcer::Member
        );
    }

    /// One window, one spelling — so a member's applied value compares equal to
    /// the Keeper's — and the Keeper tells "update it" from "its operator has
    /// to choose a time zone" by the release the member reports.
    #[test]
    fn night_watch_hours_take_one_spelling_and_a_gap_says_which_fix() {
        let hours = parse_daily_hours("22:00-07:00").unwrap();
        assert_eq!((hours.start_minute(), hours.end_minute()), (1_320, 420));
        for wrong in [
            "22:00",
            "22:00-22:00",
            "9:00-17:00",
            "24:00-07:00",
            "22:60-07:00",
            "22-07",
        ] {
            assert!(
                parse_daily_hours(wrong).is_none(),
                "{wrong:?} is not a window"
            );
        }
        assert_eq!(
            validate_required_settings(&[setting("night_watch.hours", "22:00-07:00")]),
            Ok(())
        );
        assert_eq!(
            validate_required_settings(&[setting("night_watch.hours", "late")]),
            Err(
                "night_watch.hours must be two different times of day, such as 22:00-07:00"
                    .to_owned()
            )
        );
        assert_eq!(
            required_night_watch_hours(&[setting("night_watch.hours", "22:00-07:00")]),
            Some(hours)
        );

        let required = [setting("night_watch.hours", "22:00-07:00")];
        let reported = |version| ReportedPolicy {
            revision: Some(4),
            applied: &[],
            version,
        };
        assert_eq!(
            policy_gaps(&required, 4, Some(reported("1.17.1")))[0].reason,
            PolicyGapReason::ReleaseCannotApply,
            "a release that does not know the setting is told to update"
        );
        for knows in [
            "1.17.2",
            "1.17.2-dev-3c91e83b952e-20261013212652-1780948",
            "1.18.0",
        ] {
            assert_eq!(
                policy_gaps(&required, 4, Some(reported(knows)))[0].reason,
                PolicyGapReason::NeedsLocalChoice,
                "{knows} applies it once its operator chooses a time zone"
            );
        }
    }
}
