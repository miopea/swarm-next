//! When merged work has waited too long for its release (ADR 0180).
//!
//! A task in Awaiting Release has merged, or is about to, and waits for a
//! release. Two things make that wait worth naming, and the operator chose
//! both:
//!
//! - **Holding others.** Another task that is not finished waits on this one,
//!   and 30 minutes have passed since the merge. That is the cost the incident
//!   that prompted this showed: the release itself was ordinary for its
//!   repository; the work queued behind it was not.
//! - **The slow tail.** The wait has passed this repository's own 90th
//!   percentile. Release times differ by an order of magnitude between
//!   repositories, so one threshold is wrong in every one of them at once.
//!
//! The clock starts at the later of the merge and the park. More than half of
//! the stays measured were parked before their merge; counting from the park
//! alarms on work that has not merged yet.
//!
//! A pending card that waits on the operator holds the wait (ADR 0181): the
//! operator already has it in Needs you, so neither Queen nor the releaser is
//! told. When the operator answers a card linked to the task during the stay,
//! the clock starts again from that answer, because only then is the release
//! back in the hands of Queen and the releaser.

/// How long merged work may wait while another task is recorded as waiting on
/// it, in seconds.
///
/// Thirty minutes, chosen by the operator. Of the 90 stays in 30 days that
/// had another task waiting on them, 57 waited longer than this after their
/// merge and 34 longer than two hours.
pub const HOLDING_OTHERS_SECONDS: i64 = 30 * 60;

/// The fewest measured waits a repository needs before its own 90th
/// percentile is trusted.
///
/// Below this the percentile is one or two stays, and a single slow release
/// would set it.
pub const MIN_MEASURED_WAITS: usize = 20;

/// The slow-tail threshold for a repository with fewer measured waits than
/// [`MIN_MEASURED_WAITS`], in seconds.
///
/// ⚠️ MEASURED, NOT CHOSEN. Across the Hive's 623 measured stays in the 30
/// days to 2026-11-22, 90% of waits ended within 18.8 hours and 95% within
/// 31.7 hours. A day sits between the two.
pub const UNMEASURED_TAIL_SECONDS: i64 = 24 * 60 * 60;

/// Which rule raised a release wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseWaitTrigger {
    /// Another unfinished task waits on this one.
    HoldingOthers,
    /// The wait has passed the repository's slow-tail threshold.
    SlowTail,
}

impl ReleaseWaitTrigger {
    /// The stable word stored with a raised row.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HoldingOthers => "holding_others",
            Self::SlowTail => "slow_tail",
        }
    }
}

/// When a stay's wait starts: the latest of its merge, its park, and the
/// operator's latest answer to a card linked to the task during the stay.
#[must_use]
pub const fn release_wait_started_at(
    merged_at: i64,
    parked_at: i64,
    card_answered_at: Option<i64>,
) -> i64 {
    let started = if merged_at > parked_at {
        merged_at
    } else {
        parked_at
    };
    match card_answered_at {
        Some(answered) if answered > started => answered,
        _ => started,
    }
}

/// A repository's slow-tail threshold, from its measured waits in seconds.
///
/// The nearest-rank 90th percentile: the smallest wait at or above which 10%
/// of the measured waits lie. With fewer than [`MIN_MEASURED_WAITS`] waits it
/// is [`UNMEASURED_TAIL_SECONDS`].
#[must_use]
pub fn release_wait_slow_tail_seconds(waits: &[i64]) -> i64 {
    if waits.len() < MIN_MEASURED_WAITS {
        return UNMEASURED_TAIL_SECONDS;
    }
    let mut sorted = waits.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * 9).div_ceil(10);
    sorted[rank.saturating_sub(1)]
}

/// Which rule, if any, a wait trips.
///
/// Holding others wins when both hold, because it says what the wait costs.
#[must_use]
pub const fn release_wait_trigger(
    waited_seconds: i64,
    others_waiting: bool,
    slow_tail_seconds: i64,
) -> Option<ReleaseWaitTrigger> {
    if others_waiting && waited_seconds >= HOLDING_OTHERS_SECONDS {
        Some(ReleaseWaitTrigger::HoldingOthers)
    } else if waited_seconds > slow_tail_seconds {
        Some(ReleaseWaitTrigger::SlowTail)
    } else {
        None
    }
}

/// Whole hours and minutes, for the row and the reminder.
#[must_use]
pub fn release_wait_duration(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    match (hours, minutes) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_starts_at_the_merge_when_the_task_was_parked_before_it() {
        assert_eq!(release_wait_started_at(500, 100, None), 500);
        assert_eq!(release_wait_started_at(100, 500, None), 500);
    }

    #[test]
    fn the_wait_starts_again_when_the_operator_answers_a_card_during_the_stay() {
        assert_eq!(release_wait_started_at(500, 100, Some(900)), 900);
        // An answer before the merge changes nothing: the merge is later.
        assert_eq!(release_wait_started_at(500, 100, Some(300)), 500);
    }

    #[test]
    fn a_repository_with_too_few_waits_uses_the_measured_hive_default() {
        let waits = vec![60; MIN_MEASURED_WAITS - 1];
        assert_eq!(
            release_wait_slow_tail_seconds(&waits),
            UNMEASURED_TAIL_SECONDS
        );
        assert_eq!(release_wait_slow_tail_seconds(&[]), UNMEASURED_TAIL_SECONDS);
    }

    #[test]
    fn the_slow_tail_is_the_nearest_rank_ninetieth_percentile() {
        // 1..=20 minutes, unsorted: the 18th smallest is 18 minutes.
        let waits: Vec<i64> = (1..=20).rev().map(|minutes| minutes * 60).collect();
        assert_eq!(release_wait_slow_tail_seconds(&waits), 18 * 60);
        // 1..=25: ceil(22.5) = 23rd.
        let waits: Vec<i64> = (1..=25).map(|minutes| minutes * 60).collect();
        assert_eq!(release_wait_slow_tail_seconds(&waits), 23 * 60);
    }

    #[test]
    fn work_others_wait_on_trips_at_thirty_minutes_and_not_before() {
        let tail = 10 * 60 * 60;
        assert_eq!(
            release_wait_trigger(HOLDING_OTHERS_SECONDS - 1, true, tail),
            None
        );
        assert_eq!(
            release_wait_trigger(HOLDING_OTHERS_SECONDS, true, tail),
            Some(ReleaseWaitTrigger::HoldingOthers)
        );
        assert_eq!(
            release_wait_trigger(HOLDING_OTHERS_SECONDS, false, tail),
            None,
            "nothing waits on it, so only the tail can trip it"
        );
    }

    #[test]
    fn the_slow_tail_trips_only_past_its_threshold() {
        assert_eq!(release_wait_trigger(3_600, false, 3_600), None);
        assert_eq!(
            release_wait_trigger(3_601, false, 3_600),
            Some(ReleaseWaitTrigger::SlowTail)
        );
    }

    #[test]
    fn holding_others_is_named_when_both_hold() {
        assert_eq!(
            release_wait_trigger(7_200, true, 3_600),
            Some(ReleaseWaitTrigger::HoldingOthers)
        );
    }

    #[test]
    fn durations_read_as_hours_and_minutes() {
        assert_eq!(release_wait_duration(59), "0 min");
        assert_eq!(release_wait_duration(45 * 60), "45 min");
        assert_eq!(release_wait_duration(2 * 3_600), "2 h");
        assert_eq!(release_wait_duration(3 * 3_600 + 10 * 60), "3 h 10 min");
    }
}
