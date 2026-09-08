//! Hive-owned delivery of explicitly reviewed support reports, not task intake.
use serde::{Deserialize, Serialize};

pub const SUPPORT_OUTBOX_MAX_ATTEMPTS: u32 = 5;
pub const SUPPORT_OUTBOX_MAX_ROWS: usize = 256;
pub const SUPPORT_OUTBOX_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportRetryRequest {
    pub submission_key: uuid::Uuid,
    pub retry_id: uuid::Uuid,
    pub expected_attempt_id: uuid::Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgetSupportReport {
    pub submission_key: uuid::Uuid,
    pub expected_message_id: uuid::Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportDeliveryState {
    Pending,
    Delivering,
    Uncertain,
    RateLimited,
    Failed,
    Confirmed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportDeliveryTransitionError {
    NotRetryable,
    AttemptLimit,
    NotDelivering,
    InvalidOutcome,
}

impl SupportDeliveryState {
    #[must_use]
    pub const fn may_remove_local_copy(self) -> bool {
        matches!(self, Self::Confirmed)
    }
    /// One explicit operator retry, not a reset of the automatic budget.
    ///
    /// # Errors
    /// Refuses in-flight/confirmed reports and counter overflow.
    pub fn begin_manual(
        self,
        attempts: u32,
    ) -> Result<(Self, u32), SupportDeliveryTransitionError> {
        if !matches!(self, Self::Uncertain | Self::Failed | Self::RateLimited) {
            return Err(SupportDeliveryTransitionError::NotRetryable);
        }
        let attempts = attempts
            .checked_add(1)
            .ok_or(SupportDeliveryTransitionError::AttemptLimit)?;
        Ok((Self::Delivering, attempts))
    }
    /// Starts a bounded attempt using the same frozen submission identity.
    ///
    /// # Errors
    /// In-flight/confirmed/definitively failed reports cannot start automatically.
    pub fn begin(self, attempts: u32) -> Result<(Self, u32), SupportDeliveryTransitionError> {
        if !matches!(self, Self::Pending | Self::Uncertain | Self::RateLimited) {
            return Err(SupportDeliveryTransitionError::NotRetryable);
        }
        if attempts >= SUPPORT_OUTBOX_MAX_ATTEMPTS {
            return Err(SupportDeliveryTransitionError::AttemptLimit);
        }
        Ok((Self::Delivering, attempts + 1))
    }

    /// Settles only the currently claimed attempt; adapters must fence its identity.
    ///
    /// # Errors
    /// Refuses completion without an in-flight attempt or a terminal outcome.
    pub fn settle(self, outcome: Self) -> Result<Self, SupportDeliveryTransitionError> {
        if self != Self::Delivering {
            return Err(SupportDeliveryTransitionError::NotDelivering);
        }
        if !matches!(
            outcome,
            Self::Confirmed | Self::Uncertain | Self::Failed | Self::RateLimited
        ) {
            return Err(SupportDeliveryTransitionError::InvalidOutcome);
        }
        Ok(outcome)
    }

    /// Process interruption never proves remote rejection or acceptance.
    #[must_use]
    pub const fn interrupted(self) -> Self {
        match self {
            Self::Delivering => Self::Uncertain,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_retry_does_not_reset_counts_or_allow_inflight_and_confirmed_work() {
        assert_eq!(
            SupportDeliveryState::Uncertain.begin_manual(5),
            Ok((SupportDeliveryState::Delivering, 6))
        );
        assert!(
            SupportDeliveryState::Uncertain
                .begin_manual(u32::MAX)
                .is_err()
        );
        for state in [
            SupportDeliveryState::Pending,
            SupportDeliveryState::Delivering,
            SupportDeliveryState::Confirmed,
        ] {
            assert!(state.begin_manual(5).is_err());
        }
    }

    #[test]
    fn lost_responses_keep_retryable_uncertainty_but_attempts_are_bounded() {
        let mut state = SupportDeliveryState::Pending;
        let mut attempts = 0;
        for _ in 0..SUPPORT_OUTBOX_MAX_ATTEMPTS {
            (state, attempts) = state.begin(attempts).unwrap();
            state = state.settle(SupportDeliveryState::Uncertain).unwrap();
        }
        assert_eq!(
            state.begin(attempts),
            Err(SupportDeliveryTransitionError::AttemptLimit)
        );
    }

    #[test]
    fn confirmed_failed_and_inflight_reports_do_not_start_again() {
        for state in [
            SupportDeliveryState::Confirmed,
            SupportDeliveryState::Failed,
            SupportDeliveryState::Delivering,
        ] {
            assert_eq!(
                state.begin(1),
                Err(SupportDeliveryTransitionError::NotRetryable)
            );
        }
    }

    #[test]
    fn only_inflight_reports_can_be_settled_and_restart_is_not_confirmation() {
        for state in [
            SupportDeliveryState::Pending,
            SupportDeliveryState::Uncertain,
            SupportDeliveryState::Failed,
            SupportDeliveryState::Confirmed,
        ] {
            assert_eq!(
                state.settle(SupportDeliveryState::Confirmed),
                Err(SupportDeliveryTransitionError::NotDelivering)
            );
            assert_eq!(state.interrupted(), state);
        }
        assert_eq!(
            SupportDeliveryState::Delivering.interrupted(),
            SupportDeliveryState::Uncertain
        );
        assert_eq!(
            SupportDeliveryState::Delivering.settle(SupportDeliveryState::Pending),
            Err(SupportDeliveryTransitionError::InvalidOutcome)
        );
    }
}
