//! Settings a running worker reads only when it starts, and whether its session
//! still carries an older value of one (ADR 0132).
//!
//! The operator turned on Queen releases, restarted every worker the moment
//! before, and asked "what do you mean turn it on?". Each of these settings
//! said, in a line of small print on its own card, that it reaches a worker
//! the next time that worker starts. Nothing said which workers were still
//! running without it.

/// An operator setting that reaches a worker only the next time it starts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RestartBoundSetting {
    /// The model repository workers run on (ADR 0116).
    WorkerModel,
    /// Whether Claude workers connect to Remote Control (ADR 0122).
    RemoteControl,
    /// Whether Queen's review is enough to release code (ADR 0131).
    QueenReleases,
    /// How far into the machine one worker may reach.
    SystemAccess,
}

impl RestartBoundSetting {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkerModel => "worker_model",
            Self::RemoteControl => "remote_control",
            Self::QueenReleases => "queen_releases",
            Self::SystemAccess => "system_access",
        }
    }
}

/// What Swarm knows of the value a running session started with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchedValue<T> {
    /// Nothing was recorded for this session: it started before Swarm kept
    /// a record of what each launch carried.
    Unrecorded,
    /// Swarm wrote this value beside the worker's config before it started.
    Wrote(T),
    /// Swarm could not write the value, so the session started on whatever
    /// the file held before.
    NotWritten,
}

/// Whether a running session still carries an older value of a setting it
/// reads only at start.
///
/// A recorded launch decides by value, so a setting turned on and back off
/// again asks nothing of anyone. A value Swarm could not write is behind: the
/// session is not known to carry the current one, and a restart writes it
/// again. A session from before the record is judged by time, and the same
/// second counts as behind, because which came first cannot be told apart.
#[must_use]
pub fn session_behind_setting<T: PartialEq>(
    launched: LaunchedValue<T>,
    current: &T,
    changed_at: Option<i64>,
    started_at: i64,
) -> bool {
    match launched {
        LaunchedValue::Wrote(value) => value != *current,
        LaunchedValue::NotWritten => true,
        LaunchedValue::Unrecorded => changed_at.is_some_and(|changed| changed >= started_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_launch_is_behind_only_when_its_value_differs() {
        assert!(!session_behind_setting(
            LaunchedValue::Wrote(true),
            &true,
            Some(50),
            10
        ));
        assert!(session_behind_setting(
            LaunchedValue::Wrote(false),
            &true,
            Some(5),
            10
        ));
        // A value Swarm could not write is not known to be the current one.
        assert!(session_behind_setting(
            LaunchedValue::NotWritten,
            &false,
            None,
            10
        ));
    }

    #[test]
    fn a_session_from_before_the_record_is_judged_by_time() {
        let unrecorded = LaunchedValue::<bool>::Unrecorded;
        assert!(!session_behind_setting(unrecorded, &true, None, 10));
        assert!(!session_behind_setting(unrecorded, &true, Some(9), 10));
        assert!(session_behind_setting(unrecorded, &true, Some(10), 10));
        assert!(session_behind_setting(unrecorded, &true, Some(11), 10));
    }

    #[test]
    fn each_setting_has_its_own_name() {
        let names = [
            RestartBoundSetting::WorkerModel,
            RestartBoundSetting::RemoteControl,
            RestartBoundSetting::QueenReleases,
            RestartBoundSetting::SystemAccess,
        ]
        .map(RestartBoundSetting::as_str);
        assert_eq!(
            names,
            [
                "worker_model",
                "remote_control",
                "queen_releases",
                "system_access"
            ]
        );
    }
}
