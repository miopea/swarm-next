//! How long a Hive keeps its terminal history (ADR 0118).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Days of terminal history a Hive keeps. Seven is every Hive's starting point,
/// as it was before the setting existed. A total size cap applies as well, so
/// a busy Hive may keep less than this.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct HistoryRetentionDays(u16);

impl HistoryRetentionDays {
    pub const CHOICES: [u16; 5] = [1, 3, 7, 14, 30];
    pub const DEFAULT: Self = Self(7);

    /// # Errors
    /// Rejects a number of days that is not one of the choices.
    pub fn new(days: u16) -> Result<Self, String> {
        if Self::CHOICES.contains(&days) {
            Ok(Self(days))
        } else {
            Err(format!(
                "terminal history is kept for 1, 3, 7, 14 or 30 days, not {days}"
            ))
        }
    }

    #[must_use]
    pub const fn days(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn seconds(self) -> u64 {
        self.0 as u64 * 86_400
    }
}

impl Default for HistoryRetentionDays {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<u16> for HistoryRetentionDays {
    type Error = String;
    fn try_from(days: u16) -> Result<Self, Self::Error> {
        Self::new(days)
    }
}

impl From<HistoryRetentionDays> for u16 {
    fn from(days: HistoryRetentionDays) -> Self {
        days.0
    }
}

impl fmt::Display for HistoryRetentionDays {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl FromStr for HistoryRetentionDays {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u16>()
            .map_err(|_| format!("{value:?} is not a number of days"))
            .and_then(Self::new)
    }
}

/// What the terminal host says it is using, as last read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HostRetention {
    /// Not looked at yet.
    Unknown,
    /// The host has never said: it is older than this setting, and applies it
    /// after the worker engine update.
    Silent,
    /// The host is keeping history this long.
    Using { max_age_seconds: u64 },
}

impl HostRetention {
    /// Whether the host is keeping history for exactly `wanted`.
    #[must_use]
    pub const fn keeps(self, wanted: HistoryRetentionDays) -> bool {
        matches!(self, Self::Using { max_age_seconds } if max_age_seconds == wanted.seconds())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_offered_days_are_accepted() {
        for days in HistoryRetentionDays::CHOICES {
            assert_eq!(HistoryRetentionDays::new(days).unwrap().days(), days);
            assert_eq!(
                days.to_string()
                    .parse::<HistoryRetentionDays>()
                    .unwrap()
                    .days(),
                days
            );
        }
        assert!(HistoryRetentionDays::new(0).is_err());
        assert!(HistoryRetentionDays::new(2).is_err());
        assert!("week".parse::<HistoryRetentionDays>().is_err());
        assert_eq!(HistoryRetentionDays::default().seconds(), 7 * 86_400);
        assert_eq!(
            serde_json::from_str::<HistoryRetentionDays>("14")
                .unwrap()
                .days(),
            14
        );
        assert!(serde_json::from_str::<HistoryRetentionDays>("5").is_err());
    }

    #[test]
    fn in_effect_means_the_host_says_it_keeps_exactly_that() {
        let three = HistoryRetentionDays::new(3).unwrap();
        assert!(
            HostRetention::Using {
                max_age_seconds: 3 * 86_400
            }
            .keeps(three)
        );
        assert!(
            !HostRetention::Using {
                max_age_seconds: 7 * 86_400
            }
            .keeps(three)
        );
        assert!(!HostRetention::Silent.keeps(three));
        assert!(!HostRetention::Unknown.keeps(three));
    }
}
