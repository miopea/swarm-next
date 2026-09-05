use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Hourly counts only; no individual event times or free-form metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingAggregate {
    pub count: u32,
    pub total_ms: u64,
    pub max_ms: u32,
}

impl TimingAggregate {
    /// Cumulative updates may append samples, never rewrite existing samples.
    #[must_use]
    pub fn extends(self, previous: Self) -> bool {
        if !self.valid()
            || !previous.valid()
            || self.count < previous.count
            || self.total_ms < previous.total_ms
            || self.max_ms < previous.max_ms
        {
            return false;
        }
        let added = self.count - previous.count;
        let duration = self.total_ms - previous.total_ms;
        if added == 0 {
            return self == previous;
        }
        duration <= u64::from(added) * u64::from(self.max_ms)
            && (self.max_ms == previous.max_ms || duration >= u64::from(self.max_ms))
    }

    #[must_use]
    pub fn valid(self) -> bool {
        if self.count == 0 {
            return self.total_ms == 0 && self.max_ms == 0;
        }
        self.count <= 1_000_000
            && self.max_ms <= 86_400_000
            && self.total_ms >= u64::from(self.max_ms)
            && self.total_ms <= u64::from(self.count) * u64::from(self.max_ms)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserEvidenceHour {
    pub capture_id: Uuid,
    pub build: String,
    pub hour: i64,
    pub revision: u32,
    pub long_task: TimingAggregate,
    pub interaction: TimingAggregate,
    pub route: TimingAggregate,
    pub terminal_render: TimingAggregate,
    pub terminal_reconnect: TimingAggregate,
    // Existing retained captures contain no phase samples. The persistence
    // reader owns these defaults for the 90-day historical retention window.
    #[serde(default)]
    pub terminal_grant: TimingAggregate,
    #[serde(default)]
    pub terminal_socket: TimingAggregate,
    #[serde(default)]
    pub terminal_restore: TimingAggregate,
}

impl BrowserEvidenceHour {
    /// Validate stored evidence without applying the shorter upload window.
    #[must_use]
    pub fn valid(&self) -> bool {
        !self.capture_id.is_nil()
            && !self.build.is_empty()
            && self.build.len() <= 128
            && self
                .build
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_+".contains(&byte))
            && self.hour >= 0
            && self.hour % 3_600 == 0
            && self.revision > 0
            && self.metrics().into_iter().all(TimingAggregate::valid)
    }

    fn metrics(&self) -> [TimingAggregate; 8] {
        [
            self.long_task,
            self.interaction,
            self.route,
            self.terminal_render,
            self.terminal_reconnect,
            self.terminal_grant,
            self.terminal_socket,
            self.terminal_restore,
        ]
    }

    /// A retry is identical; a replacement preserves identity and all samples.
    #[must_use]
    pub fn extends(&self, previous: &Self) -> bool {
        self.valid()
            && previous.valid()
            && self.capture_id == previous.capture_id
            && self.build == previous.build
            && self.hour == previous.hour
            && self.revision > previous.revision
            && self
                .metrics()
                .into_iter()
                .zip(previous.metrics())
                .all(|(current, prior)| current.extends(prior))
    }

    /// Input validity, including the bounded offline window and clock agreement.
    #[must_use]
    pub fn valid_at(&self, now: i64) -> bool {
        self.valid() && self.hour <= now && self.hour >= now.saturating_sub(86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> BrowserEvidenceHour {
        BrowserEvidenceHour {
            capture_id: Uuid::from_u128(1),
            build: "1.4.1-dev-abc".into(),
            hour: 172_800,
            revision: 1,
            long_task: TimingAggregate::default(),
            interaction: TimingAggregate::default(),
            route: TimingAggregate::default(),
            terminal_render: TimingAggregate::default(),
            terminal_reconnect: TimingAggregate::default(),
            terminal_grant: TimingAggregate::default(),
            terminal_socket: TimingAggregate::default(),
            terminal_restore: TimingAggregate::default(),
        }
    }

    #[test]
    fn old_captures_have_no_phase_samples_and_phase_growth_is_validated() {
        let mut wire = serde_json::to_value(capture()).unwrap();
        for name in ["terminal_grant", "terminal_socket", "terminal_restore"] {
            wire.as_object_mut().unwrap().remove(name);
        }
        let old: BrowserEvidenceHour = serde_json::from_value(wire).unwrap();
        assert_eq!(old.terminal_grant, TimingAggregate::default());
        assert_eq!(old.terminal_socket, TimingAggregate::default());
        assert_eq!(old.terminal_restore, TimingAggregate::default());
        for field in 0..3 {
            let mut next = old.clone();
            next.revision += 1;
            let timing = match field {
                0 => &mut next.terminal_grant,
                1 => &mut next.terminal_socket,
                _ => &mut next.terminal_restore,
            };
            *timing = TimingAggregate {
                count: 1,
                total_ms: 20,
                max_ms: 20,
            };
            assert!(next.extends(&old));
            let mut regression = old.clone();
            regression.revision = next.revision + 1;
            assert!(!regression.extends(&next));
        }
    }

    #[test]
    fn upload_window_does_not_invalidate_retained_history() {
        let evidence = capture();
        assert!(evidence.valid_at(evidence.hour));
        assert!(evidence.valid_at(evidence.hour + 86_400));
        assert!(!evidence.valid_at(evidence.hour + 86_401));
        assert!(!evidence.valid_at(evidence.hour - 1));
        assert!(evidence.valid());
        for build in ["", "private/path", "label with spaces"] {
            let mut invalid = evidence.clone();
            invalid.build = build.into();
            assert!(!invalid.valid());
        }
    }

    #[test]
    fn replacement_preserves_identity_and_cumulative_samples() {
        let before = capture();
        let mut after = before.clone();
        after.revision = 2;
        after.route = TimingAggregate {
            count: 1,
            total_ms: 10,
            max_ms: 10,
        };
        assert!(after.extends(&before));
        assert!(!before.extends(&after));
        assert!(!after.extends(&after));
        let mut altered = after.clone();
        altered.revision = 3;
        altered.route.total_ms = 9;
        altered.route.max_ms = 9;
        assert!(!altered.extends(&after));
        for field in 0..3 {
            let mut changed = after.clone();
            match field {
                0 => changed.capture_id = Uuid::from_u128(2),
                1 => changed.build = "other-build".into(),
                _ => changed.hour += 3_600,
            }
            assert!(!changed.extends(&before));
        }
    }

    #[test]
    fn aggregate_growth_requires_possible_appended_samples() {
        let before = TimingAggregate {
            count: 2,
            total_ms: 15,
            max_ms: 10,
        };
        assert!(before.extends(before));
        assert!(
            TimingAggregate {
                count: 3,
                total_ms: 35,
                max_ms: 20
            }
            .extends(before)
        );
        assert!(
            !TimingAggregate {
                count: 3,
                total_ms: 20,
                max_ms: 20
            }
            .extends(before)
        );
        assert!(
            !TimingAggregate {
                count: 3,
                total_ms: 40,
                max_ms: 20
            }
            .extends(before)
        );
        assert!(
            !TimingAggregate {
                count: 2,
                total_ms: 16,
                max_ms: 10
            }
            .extends(before)
        );
    }

    #[test]
    fn aggregates_reject_impossible_or_unbounded_counts() {
        assert!(TimingAggregate::default().valid());
        assert!(
            TimingAggregate {
                count: 2,
                total_ms: 15,
                max_ms: 10
            }
            .valid()
        );
        for aggregate in [
            TimingAggregate {
                count: 0,
                total_ms: 1,
                max_ms: 1,
            },
            TimingAggregate {
                count: 1,
                total_ms: 20,
                max_ms: 10,
            },
            TimingAggregate {
                count: 2,
                total_ms: 5,
                max_ms: 10,
            },
            TimingAggregate {
                count: 1_000_001,
                total_ms: 1,
                max_ms: 1,
            },
        ] {
            assert!(!aggregate.valid());
        }
        assert!(
            serde_json::from_str::<TimingAggregate>(
                r#"{"count":1,"total_ms":1,"max_ms":1,"prompt":"private"}"#
            )
            .is_err()
        );
    }
}
