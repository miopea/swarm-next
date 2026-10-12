//! What a member Hive has spent, as it reports it to its Keeper (ADR 0114
//! addendum): provider tokens and worker-busy time, by UTC day and workspace.
//!
//! Totals only, never per message. Workspace names travel with the figures,
//! under the same "share as is" ruling as the member's log (ADR 0112).

use serde::{Deserialize, Serialize};

/// The report's shape. A Keeper refuses one it does not know.
pub const HIVE_USAGE_SCHEMA_VERSION: u16 = 1;
/// How many days back a member reports.
pub const USAGE_REPORT_DAYS: i64 = 30;
/// The most rows of each kind one report may carry.
pub const MAX_USAGE_ROWS: usize = 4_096;
/// The longest workspace or model name kept, in bytes.
pub const MAX_USAGE_LABEL_BYTES: usize = 512;
/// The most busy time one workspace can log in a day: a day for each of as
/// many workers as a Hive reports (256). It is worker time, summed across the
/// workspace's workers, so it can pass a day.
pub const MAX_ACTIVE_SECONDS_PER_DAY: i64 = 86_400 * 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveUsageReport {
    pub schema_version: u16,
    pub observed_at: i64,
    pub tokens: Vec<UsageTokens>,
    pub active: Vec<UsageActive>,
}

/// One workspace's provider tokens on one UTC day, for one model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageTokens {
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    pub workspace: String,
    pub model: String,
    pub input_tokens: i64,
    pub cache_write_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
    pub messages: i64,
}

/// How long a workspace's workers were busy on one UTC day, summed across
/// them: two workers busy for an hour is two hours.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageActive {
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    pub workspace: String,
    pub seconds: i64,
}

impl HiveUsageReport {
    /// Whether this report is within the bounds both sides agree on.
    ///
    /// # Errors
    /// Names the first bound the report breaks.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != HIVE_USAGE_SCHEMA_VERSION {
            return Err("unsupported usage report version");
        }
        if self.tokens.len() > MAX_USAGE_ROWS || self.active.len() > MAX_USAGE_ROWS {
            return Err("too many usage rows");
        }
        let label = |text: &str| !text.is_empty() && text.len() <= MAX_USAGE_LABEL_BYTES;
        if !self.tokens.iter().all(|row| {
            is_day(&row.day)
                && label(&row.workspace)
                && label(&row.model)
                && [
                    row.input_tokens,
                    row.cache_write_tokens,
                    row.cache_read_tokens,
                    row.output_tokens,
                    row.messages,
                ]
                .iter()
                .all(|count| *count >= 0)
        }) {
            return Err("a token row is malformed");
        }
        if !self.active.iter().all(|row| {
            is_day(&row.day)
                && label(&row.workspace)
                && (0..=MAX_ACTIVE_SECONDS_PER_DAY).contains(&row.seconds)
        }) {
            return Err("an active-time row is malformed");
        }
        Ok(())
    }
}

/// `YYYY-MM-DD`, which also sorts as a date.
fn is_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> HiveUsageReport {
        HiveUsageReport {
            schema_version: HIVE_USAGE_SCHEMA_VERSION,
            observed_at: 1,
            tokens: vec![UsageTokens {
                day: "2026-10-12".to_owned(),
                workspace: "/home/cora/projects/budgetbug".to_owned(),
                model: "claude-opus-5-5".to_owned(),
                input_tokens: 10,
                cache_write_tokens: 20,
                cache_read_tokens: 30,
                output_tokens: 40,
                messages: 2,
            }],
            active: vec![UsageActive {
                day: "2026-10-12".to_owned(),
                workspace: "/home/cora/projects/budgetbug".to_owned(),
                seconds: 3_600,
            }],
        }
    }

    #[test]
    fn a_report_within_bounds_is_accepted_and_each_bound_is_named() {
        assert_eq!(report().validate(), Ok(()));

        let mut wrong = report();
        wrong.schema_version = 2;
        assert_eq!(wrong.validate(), Err("unsupported usage report version"));

        let mut wrong = report();
        wrong.tokens[0].day = "12/10/2026".to_owned();
        assert_eq!(wrong.validate(), Err("a token row is malformed"));

        let mut wrong = report();
        wrong.tokens[0].output_tokens = -1;
        assert_eq!(wrong.validate(), Err("a token row is malformed"));

        let mut wrong = report();
        wrong.active[0].seconds = MAX_ACTIVE_SECONDS_PER_DAY + 1;
        assert_eq!(wrong.validate(), Err("an active-time row is malformed"));

        let mut wrong = report();
        wrong.active = vec![wrong.active[0].clone(); MAX_USAGE_ROWS + 1];
        assert_eq!(wrong.validate(), Err("too many usage rows"));
    }
}
