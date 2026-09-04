//! Night Watch policy. Calendar/time-zone conversion belongs to the clock adapter;
//! this module receives a local day ordinal and minute, never the server's zone.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NightWatchSchedule {
    start_minute: u16,
    end_minute: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NightWatchScheduleError {
    InvalidMinute,
    EmptyWindow,
}

impl NightWatchSchedule {
    /// Constructs one daily local-time window. Equal times are rejected rather
    /// than silently interpreted as a full-day watch.
    ///
    /// # Errors
    /// Rejects minutes outside the day and empty windows.
    pub fn new(start_minute: u16, end_minute: u16) -> Result<Self, NightWatchScheduleError> {
        if start_minute >= 1_440 || end_minute >= 1_440 {
            return Err(NightWatchScheduleError::InvalidMinute);
        }
        if start_minute == end_minute {
            return Err(NightWatchScheduleError::EmptyWindow);
        }
        Ok(Self {
            start_minute,
            end_minute,
        })
    }

    /// Identifies the occurrence by its starting local date, including the
    /// after-midnight portion. Start is inclusive; end is exclusive. Repeated
    /// local minutes during a DST fold retain the same occurrence identity.
    #[must_use]
    pub fn occurrence(self, local_day: i64, minute: u16) -> Option<i64> {
        if minute >= 1_440 {
            return None;
        }
        if self.start_minute < self.end_minute {
            (minute >= self.start_minute && minute < self.end_minute).then_some(local_day)
        } else if minute >= self.start_minute {
            Some(local_day)
        } else if minute < self.end_minute {
            local_day.checked_sub(1)
        } else {
            None
        }
    }
}

/// Bounded durable policy state: manual intent and at most one dismissed window.
/// Adapters must persist transitions together with their presence-change event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NightWatchPolicy {
    pub manual: bool,
    pub dismissed_occurrence: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NightWatchCommand {
    Enable,
    Disable,
    /// Explicit desktop app entry or interaction, not an active heartbeat,
    /// lock/unlock observation alone, or mobile interaction.
    DesktopReturn,
    /// Re-enable the configured schedule after an explicit operator choice.
    Automatic,
}

impl NightWatchPolicy {
    #[must_use]
    pub fn is_active(self, occurrence: Option<i64>) -> bool {
        self.manual || occurrence.is_some_and(|id| self.dismissed_occurrence != Some(id))
    }

    #[must_use]
    pub fn transition(self, command: NightWatchCommand, occurrence: Option<i64>) -> Self {
        match command {
            NightWatchCommand::Enable => Self {
                manual: true,
                ..self
            },
            NightWatchCommand::Disable | NightWatchCommand::DesktopReturn => Self {
                manual: false,
                dismissed_occurrence: occurrence.or(self.dismissed_occurrence),
            },
            NightWatchCommand::Automatic => Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_validates_bounds_and_ambiguous_full_day() {
        assert_eq!(
            NightWatchSchedule::new(1_440, 0),
            Err(NightWatchScheduleError::InvalidMinute)
        );
        assert_eq!(
            NightWatchSchedule::new(0, 1_440),
            Err(NightWatchScheduleError::InvalidMinute)
        );
        assert_eq!(
            NightWatchSchedule::new(60, 60),
            Err(NightWatchScheduleError::EmptyWindow)
        );
    }

    #[test]
    fn overnight_window_has_one_identity_and_exact_boundaries() {
        let schedule = NightWatchSchedule::new(22 * 60, 7 * 60).unwrap();
        assert_eq!(schedule.occurrence(100, 21 * 60 + 59), None);
        assert_eq!(schedule.occurrence(100, 22 * 60), Some(100));
        assert_eq!(schedule.occurrence(101, 0), Some(100));
        assert_eq!(schedule.occurrence(101, 7 * 60 - 1), Some(100));
        assert_eq!(schedule.occurrence(101, 7 * 60), None);
        assert_eq!(schedule.occurrence(101, 1_440), None);
    }

    #[test]
    fn same_day_window_does_not_wrap() {
        let schedule = NightWatchSchedule::new(60, 120).unwrap();
        assert_eq!(schedule.occurrence(100, 59), None);
        assert_eq!(schedule.occurrence(100, 60), Some(100));
        assert_eq!(schedule.occurrence(100, 120), None);
    }

    #[test]
    fn desktop_return_ends_manual_and_scheduled_watch_until_next_occurrence() {
        let watch = NightWatchPolicy::default().transition(NightWatchCommand::Enable, Some(100));
        assert!(watch.is_active(Some(100)));
        let returned = watch.transition(NightWatchCommand::DesktopReturn, Some(100));
        assert!(!returned.is_active(Some(100)));
        assert!(!returned.is_active(None));
        assert!(returned.is_active(Some(101)));
        // Repeated heartbeat evaluations do not modify intent or re-enter.
        assert!(!returned.is_active(Some(100)));
    }

    #[test]
    fn repeated_local_hour_cannot_reenter_a_dismissed_watch() {
        let schedule = NightWatchSchedule::new(22 * 60, 7 * 60).unwrap();
        let occurrence = schedule.occurrence(101, 90);
        let returned =
            NightWatchPolicy::default().transition(NightWatchCommand::DesktopReturn, occurrence);
        for minute in [119, 60, 90, 120, 419] {
            assert!(!returned.is_active(schedule.occurrence(101, minute)));
        }
    }

    #[test]
    fn manual_control_works_outside_schedule_and_can_restore_automatic() {
        let policy = NightWatchPolicy::default().transition(NightWatchCommand::Enable, None);
        assert!(policy.is_active(None));
        let stopped = policy.transition(NightWatchCommand::Disable, None);
        assert!(!stopped.is_active(None));
        let dismissed = policy.transition(NightWatchCommand::Disable, Some(100));
        assert!(!dismissed.is_active(Some(100)));
        assert!(
            dismissed
                .transition(NightWatchCommand::Automatic, Some(100))
                .is_active(Some(100))
        );
    }
}
