//! A member Hive's own account of its health, as it sends it to its Keeper, and
//! the status the Keeper's roster shows for each Hive (ADR 0114).
//!
//! ⚠️ EVERY CONDITION NAMES AN ACTION. The operator's rule for this whole
//! feature: "we don't want to generate too much random noise for the sake of
//! data that we wouldn't act upon". So a Hive with nothing wrong has no
//! conditions at all, and nothing here is collected only to be displayed.

use serde::{Deserialize, Serialize};

use crate::{FederationSyncCondition, ResourcePressure, VersionStanding, WorkerModel};

/// The report's shape. A Keeper refuses one it does not know rather than
/// guessing at it.
pub const HIVE_HEALTH_SCHEMA_VERSION: u16 = 1;
/// The longest sync step name kept, in bytes. The member already cuts it to 80.
pub const MAX_HEALTH_STEP_BYTES: usize = 80;
/// How long a Hive may go unheard before it reads as quiet. The same ten
/// minutes the roster has used since 2026-09-30.
pub const MEMBER_QUIET_SECONDS: i64 = 600;
/// How long a member's sync must have been failing before the Keeper is pushed
/// about it. Shorter failures recover by themselves on the retry backoff (ADR
/// 0111) and would only be noise.
pub const SUSTAINED_FAILURE_SECONDS: i64 = 600;
/// The width of one slot in a Hive's contact history.
pub const CONTACT_SLOT_SECONDS: i64 = 300;
/// How far back a Hive's contact history reaches.
pub const CONTACT_HISTORY_SECONDS: i64 = 86_400;

/// What a member says about itself. Content-free: no paths, no task or worker
/// names, nothing typed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveHealthReport {
    pub schema_version: u16,
    /// When the member put this together, on its own clock.
    pub observed_at: i64,
    pub sync: ReportedSync,
    pub engine: ReportedEngine,
    pub disk: ReportedDisk,
    pub memory: ResourcePressure,
    /// Workers holding work they have not moved in half an hour, or were never
    /// briefed on (ADR 0039).
    pub stuck_workers: u32,
    /// Why this Hive cannot read CI at all while its check gate is on: gh is
    /// missing or signed out (ADR 0115). A reason in fixed words, never a path
    /// or a repository. Absent from releases before the gate, which is not a
    /// fault, so an older member and an older Keeper both read on unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_unreadable: Option<String>,
    /// Workers still running on another model than the Hive's worker model,
    /// which they keep until they next start (ADR 0116). Absent when there are
    /// none, and from releases before the setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers_off_model: Option<WorkersOffModel>,
}

/// Running Claude repository workers that started on another model than the
/// one their Hive names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkersOffModel {
    pub count: u32,
    /// The model they switch to when they next start.
    pub model: WorkerModel,
}

/// The member's Apiary sync, as it last recorded it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportedSync {
    pub condition: FederationSyncCondition,
    pub failed_step: Option<String>,
    pub last_success_at: Option<i64>,
    pub consecutive_failures: u32,
}

/// The member's worker engine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportedEngine {
    /// A newer engine is installed and the running one has not swapped to it.
    pub update_pending: bool,
    /// How the most recent swap ended, if one was tried.
    pub last_attempt: Option<EngineAttemptOutcome>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineAttemptOutcome {
    Succeeded,
    Failed,
    TimedOut,
    /// Started and never reported an ending.
    Unfinished,
}

/// The member's tightest storage (ADR 0083's three roles).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReportedDisk {
    pub pressure: ResourcePressure,
    pub lowest_available_bytes: Option<u64>,
}

impl HiveHealthReport {
    /// Whether this report is within the bounds both sides agree on.
    ///
    /// # Errors
    /// Names the first bound the report breaks.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != HIVE_HEALTH_SCHEMA_VERSION {
            return Err("unsupported health report version");
        }
        if self
            .sync
            .failed_step
            .as_ref()
            .is_some_and(|step| step.is_empty() || step.len() > MAX_HEALTH_STEP_BYTES)
        {
            return Err("the failed step is empty or too long");
        }
        if self
            .ci_unreadable
            .as_ref()
            .is_some_and(|why| why.is_empty() || why.len() > MAX_HEALTH_STEP_BYTES)
        {
            return Err("the CI reason is empty or too long");
        }
        if self
            .workers_off_model
            .is_some_and(|off| off.count == 0 || off.model.alias().is_none())
        {
            return Err("workers off the model must name a model and at least one worker");
        }
        Ok(())
    }
}

/// One thing wrong with a Hive, worst first when there are several. Each is
/// also its fix, which the screen says beside it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HiveCondition {
    /// Not heard from in ten minutes. Never pushed: the 2026-09-30 ruling is
    /// to warn about silence when the operator goes to use the Hive.
    Quiet {
        last_contact_at: Option<i64>,
    },
    /// Its Apiary sync is stopping at a step.
    FailingSync {
        step: Option<String>,
        /// The last time a sync got all the way through, if ever.
        last_success_at: Option<i64>,
        /// Failing for long enough that retrying alone has not fixed it.
        sustained: bool,
    },
    /// Its database is behind the Apiary's release, which stops shared work.
    SchemaBehind {
        running: String,
    },
    Behind {
        running: String,
    },
    /// A newer engine is waiting and the last swap to one failed.
    EngineUpdateFailing,
    LowDisk {
        lowest_available_bytes: Option<u64>,
        critical: bool,
    },
    ShortOfMemory,
    WorkersStuck {
        count: u32,
    },
    /// Its check gate is on but it cannot read CI at all, so the work it
    /// closes goes unchecked (ADR 0115).
    CiUnreadable {
        why: String,
    },
    /// Running workers that started on another model than the one the Hive
    /// names; each switches when it next starts (ADR 0116). Never a remote
    /// restart, so the fix is its operator's.
    WorkersOffModel {
        count: u32,
        model: WorkerModel,
    },
    /// A newer engine is waiting; it swaps by itself once workers are idle.
    EngineUpdatePending,
}

impl HiveCondition {
    /// Whether this is a fault the Keeper is pushed about: failing sync that
    /// retrying has not fixed, an engine update that will not apply, or disk
    /// about to run out. Schema behind is pushed already, by the fleet's
    /// "Hives are behind" question. Never quiet.
    #[must_use]
    pub const fn pushes(&self) -> bool {
        matches!(
            self,
            Self::FailingSync {
                sustained: true,
                ..
            } | Self::EngineUpdateFailing
                | Self::LowDisk { critical: true, .. }
        )
    }

    /// A stable name for this kind of condition, whatever its details.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Quiet { .. } => "quiet",
            Self::FailingSync { .. } => "failing_sync",
            Self::SchemaBehind { .. } => "schema_behind",
            Self::Behind { .. } => "behind",
            Self::EngineUpdateFailing => "engine_update_failing",
            Self::LowDisk { .. } => "low_disk",
            Self::ShortOfMemory => "short_of_memory",
            Self::WorkersStuck { .. } => "workers_stuck",
            Self::CiUnreadable { .. } => "ci_unreadable",
            Self::WorkersOffModel { .. } => "workers_off_model",
            Self::EngineUpdatePending => "engine_update_pending",
        }
    }
}

/// What the Keeper knows about one member Hive.
#[derive(Clone, Copy, Debug)]
pub struct HiveHealthEvidence<'a> {
    /// When the Hive last reached the Keeper, if ever.
    pub last_contact_at: Option<i64>,
    /// How its reported release stands against the Apiary's.
    pub version: Option<(VersionStanding, &'a str)>,
    /// What the Hive last said about itself. Absent from a release before
    /// this report existed, which is not itself a fault.
    pub health: Option<&'a HiveHealthReport>,
}

/// Everything wrong with one Hive, worst first. Empty when it is healthy.
///
/// ⚠️ A QUIET HIVE'S OWN REPORT IS NOT BELIEVED. It is what the Hive said
/// before it went quiet, so it says nothing about now; only its release, which
/// cannot change without the Hive reporting, still stands beside it.
#[must_use]
pub fn hive_conditions(evidence: &HiveHealthEvidence<'_>, now: i64) -> Vec<HiveCondition> {
    let mut found = Vec::new();
    let quiet = evidence
        .last_contact_at
        .is_none_or(|at| now.saturating_sub(at) >= MEMBER_QUIET_SECONDS);
    if quiet {
        found.push(HiveCondition::Quiet {
            last_contact_at: evidence.last_contact_at,
        });
    }
    let health = evidence.health.filter(|_| !quiet);
    if let Some(sync) = health.map(|health| &health.sync)
        && !matches!(
            sync.condition,
            FederationSyncCondition::Current | FederationSyncCondition::Idle
        )
    {
        found.push(HiveCondition::FailingSync {
            step: sync.failed_step.clone(),
            last_success_at: sync.last_success_at,
            sustained: sync
                .last_success_at
                .is_none_or(|at| now.saturating_sub(at) >= SUSTAINED_FAILURE_SECONDS),
        });
    }
    match evidence.version {
        Some((VersionStanding::SchemaBehind, running)) => found.push(HiveCondition::SchemaBehind {
            running: running.to_owned(),
        }),
        Some((VersionStanding::Behind, running)) => found.push(HiveCondition::Behind {
            running: running.to_owned(),
        }),
        _ => {}
    }
    let Some(health) = health else {
        return found;
    };
    let engine_failing = health.engine.update_pending
        && matches!(
            health.engine.last_attempt,
            Some(EngineAttemptOutcome::Failed | EngineAttemptOutcome::TimedOut)
        );
    if engine_failing {
        found.push(HiveCondition::EngineUpdateFailing);
    }
    if matches!(
        health.disk.pressure,
        ResourcePressure::Advisory | ResourcePressure::Critical
    ) {
        found.push(HiveCondition::LowDisk {
            lowest_available_bytes: health.disk.lowest_available_bytes,
            critical: health.disk.pressure == ResourcePressure::Critical,
        });
    }
    if health.memory == ResourcePressure::Critical {
        found.push(HiveCondition::ShortOfMemory);
    }
    if health.stuck_workers > 0 {
        found.push(HiveCondition::WorkersStuck {
            count: health.stuck_workers,
        });
    }
    if let Some(why) = &health.ci_unreadable {
        found.push(HiveCondition::CiUnreadable { why: why.clone() });
    }
    if let Some(off) = health.workers_off_model {
        found.push(HiveCondition::WorkersOffModel {
            count: off.count,
            model: off.model,
        });
    }
    if health.engine.update_pending && !engine_failing {
        found.push(HiveCondition::EngineUpdatePending);
    }
    found
}

/// The start of the contact-history slot `at` falls in.
#[must_use]
pub const fn contact_slot(at: i64) -> i64 {
    at - at.rem_euclid(CONTACT_SLOT_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn healthy() -> HiveHealthReport {
        HiveHealthReport {
            schema_version: HIVE_HEALTH_SCHEMA_VERSION,
            observed_at: NOW,
            sync: ReportedSync {
                condition: FederationSyncCondition::Current,
                failed_step: None,
                last_success_at: Some(NOW - 30),
                consecutive_failures: 0,
            },
            engine: ReportedEngine {
                update_pending: false,
                last_attempt: Some(EngineAttemptOutcome::Succeeded),
            },
            disk: ReportedDisk {
                pressure: ResourcePressure::Normal,
                lowest_available_bytes: Some(40 << 30),
            },
            memory: ResourcePressure::Normal,
            stuck_workers: 0,
            ci_unreadable: None,
            workers_off_model: None,
        }
    }

    fn evidence(health: &HiveHealthReport) -> HiveHealthEvidence<'_> {
        HiveHealthEvidence {
            last_contact_at: Some(NOW - 20),
            version: Some((VersionStanding::Current, "1.16.10")),
            health: Some(health),
        }
    }

    #[test]
    fn a_healthy_hive_has_no_conditions_and_shows_nothing_extra() {
        let health = healthy();
        assert_eq!(hive_conditions(&evidence(&health), NOW), Vec::new());
        // A release from before the report existed is not itself a fault.
        let older = HiveHealthEvidence {
            health: None,
            ..evidence(&health)
        };
        assert_eq!(hive_conditions(&older, NOW), Vec::new());
    }

    #[test]
    fn each_condition_is_found_worst_first_and_only_faults_push() {
        let mut health = healthy();
        health.sync = ReportedSync {
            condition: FederationSyncCondition::Incompatible,
            failed_step: Some("project catalog".to_owned()),
            last_success_at: Some(NOW - 700),
            consecutive_failures: 6,
        };
        health.engine = ReportedEngine {
            update_pending: true,
            last_attempt: Some(EngineAttemptOutcome::TimedOut),
        };
        health.disk = ReportedDisk {
            pressure: ResourcePressure::Critical,
            lowest_available_bytes: Some(300 << 20),
        };
        health.memory = ResourcePressure::Critical;
        health.stuck_workers = 2;
        let found = hive_conditions(
            &HiveHealthEvidence {
                version: Some((VersionStanding::SchemaBehind, "1.15.0")),
                ..evidence(&health)
            },
            NOW,
        );
        assert_eq!(
            found.iter().map(HiveCondition::key).collect::<Vec<_>>(),
            [
                "failing_sync",
                "schema_behind",
                "engine_update_failing",
                "low_disk",
                "short_of_memory",
                "workers_stuck"
            ]
        );
        assert_eq!(
            found
                .iter()
                .filter(|condition| condition.pushes())
                .map(HiveCondition::key)
                .collect::<Vec<_>>(),
            ["failing_sync", "engine_update_failing", "low_disk"]
        );
    }

    #[test]
    fn a_short_failure_is_shown_but_not_pushed_and_advisory_disk_is_not_pushed() {
        let mut health = healthy();
        health.sync.condition = FederationSyncCondition::Offline;
        health.sync.last_success_at = Some(NOW - 90);
        health.disk.pressure = ResourcePressure::Advisory;
        let found = hive_conditions(&evidence(&health), NOW);
        assert_eq!(
            found,
            [
                HiveCondition::FailingSync {
                    step: None,
                    last_success_at: Some(NOW - 90),
                    sustained: false,
                },
                HiveCondition::LowDisk {
                    lowest_available_bytes: Some(40 << 30),
                    critical: false,
                },
            ]
        );
        assert!(found.iter().all(|condition| !condition.pushes()));
    }

    #[test]
    fn a_pending_engine_is_only_pending_until_a_swap_to_it_fails() {
        let mut health = healthy();
        health.engine.update_pending = true;
        assert_eq!(
            hive_conditions(&evidence(&health), NOW),
            [HiveCondition::EngineUpdatePending]
        );
        health.engine.last_attempt = Some(EngineAttemptOutcome::Failed);
        assert_eq!(
            hive_conditions(&evidence(&health), NOW),
            [HiveCondition::EngineUpdateFailing]
        );
    }

    #[test]
    fn a_quiet_hive_is_quiet_and_its_old_report_is_not_believed() {
        let mut health = healthy();
        health.disk.pressure = ResourcePressure::Critical;
        let quiet = HiveHealthEvidence {
            last_contact_at: Some(NOW - MEMBER_QUIET_SECONDS),
            version: Some((VersionStanding::Behind, "1.15.0")),
            health: Some(&health),
        };
        assert_eq!(
            hive_conditions(&quiet, NOW),
            [
                HiveCondition::Quiet {
                    last_contact_at: Some(NOW - MEMBER_QUIET_SECONDS)
                },
                HiveCondition::Behind {
                    running: "1.15.0".to_owned()
                },
            ]
        );
        let never = HiveHealthEvidence {
            last_contact_at: None,
            version: None,
            health: None,
        };
        assert!(!hive_conditions(&never, NOW)[0].pushes());
    }

    #[test]
    fn a_report_outside_its_bounds_is_refused_by_name() {
        let mut health = healthy();
        assert_eq!(health.validate(), Ok(()));
        health.sync.failed_step = Some("x".repeat(MAX_HEALTH_STEP_BYTES + 1));
        assert_eq!(
            health.validate(),
            Err("the failed step is empty or too long")
        );
        health.sync.failed_step = None;
        health.schema_version = 2;
        assert_eq!(health.validate(), Err("unsupported health report version"));
    }

    #[test]
    fn contact_slots_are_five_minutes_wide() {
        assert_eq!(contact_slot(NOW), NOW);
        assert_eq!(contact_slot(NOW + 299), NOW);
        assert_eq!(contact_slot(NOW + 300), NOW + 300);
    }

    /// ⚠️ A HIVE WHOSE GATE CANNOT READ CI CLOSES WORK UNCHECKED, so its Keeper
    /// sees why (ADR 0115) — and the field travels both ways between releases.
    #[test]
    fn a_hive_that_cannot_read_ci_says_so_and_older_reports_still_read() {
        let report = HiveHealthReport {
            ci_unreadable: Some(crate::CI_GH_SIGNED_OUT.to_owned()),
            ..healthy()
        };
        assert_eq!(
            hive_conditions(&evidence(&report), NOW),
            vec![HiveCondition::CiUnreadable {
                why: "gh is not logged in on this machine".to_owned()
            }]
        );
        assert!(!HiveCondition::CiUnreadable { why: String::new() }.pushes());

        let mut older = serde_json::to_value(healthy()).unwrap();
        assert!(older.get("ci_unreadable").is_none(), "absent, not null");
        older.as_object_mut().unwrap().remove("ci_unreadable");
        let read: HiveHealthReport = serde_json::from_value(older).unwrap();
        assert_eq!(read.ci_unreadable, None);
        assert_eq!(
            HiveHealthReport {
                ci_unreadable: Some(String::new()),
                ..healthy()
            }
            .validate(),
            Err("the CI reason is empty or too long")
        );
    }

    /// A member whose workers have not all started on the Hive's model says
    /// how many, and the field travels both ways between releases (ADR 0116).
    #[test]
    fn workers_off_the_model_are_counted_and_older_reports_still_read() {
        let report = HiveHealthReport {
            workers_off_model: Some(WorkersOffModel {
                count: 2,
                model: WorkerModel::Sonnet,
            }),
            ..healthy()
        };
        assert_eq!(
            hive_conditions(&evidence(&report), NOW),
            vec![HiveCondition::WorkersOffModel {
                count: 2,
                model: WorkerModel::Sonnet
            }]
        );
        assert!(
            !HiveCondition::WorkersOffModel {
                count: 2,
                model: WorkerModel::Sonnet
            }
            .pushes(),
            "they switch by themselves when they next start"
        );
        assert_eq!(
            serde_json::to_value(&hive_conditions(&evidence(&report), NOW)[0]).unwrap(),
            serde_json::json!({ "kind": "workers_off_model", "count": 2, "model": "sonnet" })
        );

        let mut older = serde_json::to_value(healthy()).unwrap();
        assert!(older.get("workers_off_model").is_none(), "absent, not null");
        older.as_object_mut().unwrap().remove("workers_off_model");
        let read: HiveHealthReport = serde_json::from_value(older).unwrap();
        assert_eq!(read.workers_off_model, None);
        for nonsense in [
            WorkersOffModel {
                count: 0,
                model: WorkerModel::Sonnet,
            },
            WorkersOffModel {
                count: 1,
                model: WorkerModel::Default,
            },
        ] {
            assert!(
                HiveHealthReport {
                    workers_off_model: Some(nonsense),
                    ..healthy()
                }
                .validate()
                .is_err()
            );
        }
    }
}
