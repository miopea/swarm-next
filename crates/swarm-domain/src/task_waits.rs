//! What a blocked task is waiting for, and who must act (ADR 0119).
//!
//! A wait is named in the Blocked note, by marker lines, for the reason the
//! date hold already is: an MCP client fetches its tool list once when it
//! connects, so a new argument would be invisible to every worker already
//! running. A line in the note works from every session that exists, and the
//! refusal that teaches the format is a tool result, which they all see.

use serde::{Deserialize, Serialize};

/// The shortest wait a recheck or resume time may name.
pub const MIN_WAIT_SECONDS: i64 = 60 * 60;
/// The longest. A longer wait is blocked again when it comes back.
pub const MAX_WAIT_SECONDS: i64 = 30 * 86_400;
/// What must happen, in words.
pub const MAX_WAIT_NEEDS_BYTES: usize = 500;
/// The outside party's name.
pub const MAX_WAIT_PARTY_BYTES: usize = 120;

/// What a block waits for.
///
/// A wait on another task is still a prerequisite and a wait on an answer is
/// still a decision link, recorded where they always were. The `Task` and
/// `Decision` rows only say that THIS block waits on them. That matters
/// because the links outlive the block: a decision answered in September is
/// still linked when the task is blocked again in October for a reason nobody
/// recorded, and bringing that task back because its old decision is answered
/// would be wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskWaitKind {
    /// Another task must finish. The prerequisites say which.
    Task,
    /// The operator must answer. The linked decision says what.
    Decision,
    /// The operator must do something. It is a Needs you item until they press
    /// Done.
    OperatorAction,
    /// A named outside party must act. The worker rechecks at `resume_at`.
    Outside,
    /// Nobody can act until `resume_at`.
    Date,
}

impl TaskWaitKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Decision => "decision",
            Self::OperatorAction => "operator_action",
            Self::Outside => "outside",
            Self::Date => "date",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "task" => Some(Self::Task),
            "decision" => Some(Self::Decision),
            "operator_action" => Some(Self::OperatorAction),
            "outside" => Some(Self::Outside),
            "date" => Some(Self::Date),
            _ => None,
        }
    }
}

/// A wait named in a Blocked note.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskWait {
    pub kind: TaskWaitKind,
    /// What must happen, in words.
    pub needs: String,
    /// Who must act, for an outside wait.
    pub party: Option<String>,
    /// When the task comes back by itself, for an outside or date wait.
    pub resume_at: Option<i64>,
}

/// The current wait on a blocked task, as the board shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskWaitView {
    pub kind: TaskWaitKind,
    pub needs: String,
    pub party: Option<String>,
    pub resume_at: Option<i64>,
    /// When the operator pressed Done, for an operator action.
    pub done_at: Option<i64>,
    /// Whether it had cleared when it was read.
    pub cleared: bool,
}

impl TaskWaitView {
    /// Whether this wait has cleared at `now`, so the task may come back.
    #[must_use]
    pub fn cleared_at(&self, now: i64) -> bool {
        match self.kind {
            // Read from the gates themselves, which the board holds.
            TaskWaitKind::Task | TaskWaitKind::Decision => self.cleared,
            TaskWaitKind::OperatorAction => self.done_at.is_some(),
            TaskWaitKind::Outside | TaskWaitKind::Date => {
                self.resume_at.is_some_and(|resume| resume <= now)
            }
        }
    }
}

impl crate::NextMoveOwner {
    /// A typed wait names who must act, so a blocked task with one is never
    /// Queen's to judge (ADR 0119).
    ///
    /// - An operator action that is not done is the operator's.
    /// - An outside or date wait that has not cleared is nobody's until then.
    /// - A cleared wait is the assigned worker's, provided its prerequisites
    ///   are done. Swarm moves it back to Ready by itself, and this is what the
    ///   board says in the moment before it does.
    ///
    /// A pending decision still wins: the operator's answer is the next move.
    #[must_use]
    pub fn after_wait(
        self,
        state: crate::TaskState,
        wait: Option<&TaskWaitView>,
        assigned: bool,
        prerequisites: &[crate::TaskPrerequisite],
    ) -> Self {
        let Some(wait) = wait else {
            return self;
        };
        if state != crate::TaskState::Blocked
            || self == Self::Operator
            || matches!(wait.kind, TaskWaitKind::Task | TaskWaitKind::Decision) && !wait.cleared
        {
            return self;
        }
        if wait.cleared {
            if !prerequisites.iter().all(crate::TaskPrerequisite::satisfied) {
                return Self::Blocked;
            }
            return if assigned { Self::Worker } else { Self::Queen };
        }
        match wait.kind {
            TaskWaitKind::OperatorAction => Self::Operator,
            TaskWaitKind::Task
            | TaskWaitKind::Decision
            | TaskWaitKind::Outside
            | TaskWaitKind::Date => Self::Blocked,
        }
    }
}

/// How to name a wait, said once so every refusal teaches the same format.
pub const TASK_WAIT_FORMAT: &str = "Name the wait with marker lines in the Blocked note:\n\
     - The operator must do something:\n  Waiting on: operator\n  Needs: <what they must do>\n\
     - The operator must answer or choose: no wait line. Ask it with swarm_request_decision \
     on the task, which blocks the task until they answer.\n\
     - An outside party must act:\n  Waiting on: <who>\n  Needs: <what must happen>\n  \
     Recheck: <when to check again, e.g. 2026-10-20T13:00:00Z>\n\
     - Nothing can happen until a date:\n  Blocked until: <e.g. 2026-11-16T14:00:00Z>\n\
     Times are UTC with a Z, between one hour and 30 days ahead. The task comes back to you \
     by itself when the time arrives or the operator presses Done.";

/// Why a wait in a note could not be recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskWaitError {
    MissingNeeds,
    NeedsTooLong,
    PartyTooLong,
    OutsideNeedsRecheck,
    OperatorHasNoRecheck,
    UnreadableTime(String),
    TimeOutOfRange,
    RecheckWithoutParty,
}

impl std::fmt::Display for TaskWaitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let problem = match self {
            Self::MissingNeeds => "\"Waiting on:\" needs a \"Needs:\" line saying what must happen.".to_owned(),
            Self::NeedsTooLong => format!("\"Needs:\" is longer than {MAX_WAIT_NEEDS_BYTES} bytes."),
            Self::PartyTooLong => format!("\"Waiting on:\" is longer than {MAX_WAIT_PARTY_BYTES} bytes."),
            Self::OutsideNeedsRecheck => "An outside wait needs a \"Recheck:\" time, so it comes back to be checked.".to_owned(),
            Self::OperatorHasNoRecheck => "An operator action has no \"Recheck:\". It waits until the operator presses Done.".to_owned(),
            Self::UnreadableTime(value) => format!("\"{value}\" is not a UTC time like 2026-10-20T13:00:00Z."),
            Self::TimeOutOfRange => "The time must be between one hour and 30 days ahead.".to_owned(),
            Self::RecheckWithoutParty => "\"Recheck:\" belongs with \"Waiting on:\" an outside party. For a date alone, use \"Blocked until:\".".to_owned(),
        };
        write!(formatter, "{problem}\n\n{TASK_WAIT_FORMAT}")
    }
}

impl std::error::Error for TaskWaitError {}

fn marker<'a>(note: &'a str, key: &str) -> Option<&'a str> {
    note.lines()
        .find_map(|line| line.trim().strip_prefix(key))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn within_range(at: i64, now: i64) -> bool {
    let ahead = at.saturating_sub(now);
    (MIN_WAIT_SECONDS..=MAX_WAIT_SECONDS).contains(&ahead)
}

/// Reads the wait a Blocked note names, if it names one.
///
/// `Blocked until:` alone is the date hold that already existed. It is a typed
/// wait only inside the range; outside it, it keeps its old meaning, a hold
/// with no owner, rather than refusing notes that were accepted before.
///
/// # Errors
/// Refuses a note that starts to name a wait and gets it wrong, with the format.
pub fn parse_block_wait(note: &str, now: i64) -> Result<Option<TaskWait>, TaskWaitError> {
    let party = marker(note, "Waiting on:");
    let needs = marker(note, "Needs:");
    let recheck = marker(note, "Recheck:");
    let Some(party) = party else {
        if recheck.is_some() {
            return Err(TaskWaitError::RecheckWithoutParty);
        }
        return Ok(marker(note, "Blocked until:")
            .and_then(parse_rfc3339_seconds)
            .filter(|at| within_range(*at, now))
            .map(|at| TaskWait {
                kind: TaskWaitKind::Date,
                needs: needs.map_or_else(
                    || "Nothing can happen until this date.".to_owned(),
                    str::to_owned,
                ),
                party: None,
                resume_at: Some(at),
            }));
    };
    let needs = needs.ok_or(TaskWaitError::MissingNeeds)?;
    if needs.len() > MAX_WAIT_NEEDS_BYTES {
        return Err(TaskWaitError::NeedsTooLong);
    }
    if party.len() > MAX_WAIT_PARTY_BYTES {
        return Err(TaskWaitError::PartyTooLong);
    }
    if matches!(
        party.to_ascii_lowercase().as_str(),
        "operator" | "the operator"
    ) {
        if recheck.is_some() {
            return Err(TaskWaitError::OperatorHasNoRecheck);
        }
        return Ok(Some(TaskWait {
            kind: TaskWaitKind::OperatorAction,
            needs: needs.to_owned(),
            party: None,
            resume_at: None,
        }));
    }
    let recheck = recheck.ok_or(TaskWaitError::OutsideNeedsRecheck)?;
    let at = parse_rfc3339_seconds(recheck)
        .ok_or_else(|| TaskWaitError::UnreadableTime(recheck.to_owned()))?;
    if !within_range(at, now) {
        return Err(TaskWaitError::TimeOutOfRange);
    }
    Ok(Some(TaskWait {
        kind: TaskWaitKind::Outside,
        needs: needs.to_owned(),
        party: Some(party.to_owned()),
        resume_at: Some(at),
    }))
}

/// Seconds since the epoch for an RFC3339 instant, without pulling in a clock
/// crate for one field.
///
/// Deliberately strict: it accepts what a worker is told to write and refuses
/// the rest, because a half-understood timestamp is worse than none. Only the
/// `Z` form, because an offset that is silently ignored would shift a deadline
/// by hours in whichever direction nobody checked.
#[must_use]
pub fn parse_rfc3339_seconds(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date = date.split('-');
    let (year, month, day) = (
        date.next()?.parse::<i64>().ok()?,
        date.next()?.parse::<i64>().ok()?,
        date.next()?.parse::<i64>().ok()?,
    );
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut time = time.split(':');
    let (hour, minute, second) = (
        time.next()?.parse::<i64>().ok()?,
        time.next()?.parse::<i64>().ok()?,
        time.next()?.split('.').next()?.parse::<i64>().ok()?,
    );
    if time.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Days from the civil calendar, Howard Hinnant's algorithm. Exact for every
    // date this will ever see and it needs no dependency.
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// RFC3339 with a `Z` for a unix time: the form `parse_rfc3339_seconds`
/// accepts, so a note Swarm writes can be read back exactly.
#[must_use]
pub fn format_rfc3339_seconds(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formatted_time_reads_back_exactly() {
        for at in [0, NOW, NOW + 13 * 3_600 + 7, 951_782_400] {
            assert_eq!(parse_rfc3339_seconds(&format_rfc3339_seconds(at)), Some(at));
        }
    }

    // 2026-10-18T00:00:00Z.
    const NOW: i64 = 1_792_281_600;

    #[test]
    fn the_operator_owes_an_action() {
        let wait = parse_block_wait(
            "Phase E needs the portal.\nWaiting on: operator\nNeeds: Sign in to the Entra portal and approve M2",
            NOW,
        )
        .unwrap()
        .unwrap();
        assert_eq!(wait.kind, TaskWaitKind::OperatorAction);
        assert_eq!(wait.needs, "Sign in to the Entra portal and approve M2");
        assert_eq!((wait.party, wait.resume_at), (None, None));
    }

    #[test]
    fn an_outside_party_comes_back_to_be_rechecked() {
        let wait = parse_block_wait(
            "Waiting on: Spectrum\nNeeds: X4 back to Available\nRecheck: 2026-10-20T13:00:00Z",
            NOW,
        )
        .unwrap()
        .unwrap();
        assert_eq!(wait.kind, TaskWaitKind::Outside);
        assert_eq!(wait.party.as_deref(), Some("Spectrum"));
        assert_eq!(
            wait.resume_at,
            parse_rfc3339_seconds("2026-10-20T13:00:00Z")
        );
    }

    #[test]
    fn a_date_alone_is_a_date_wait_inside_the_range_and_the_old_hold_outside_it() {
        let wait = parse_block_wait("Blocked until: 2026-11-16T14:00:00Z", NOW)
            .unwrap()
            .unwrap();
        assert_eq!(wait.kind, TaskWaitKind::Date);
        assert_eq!(
            parse_block_wait("Blocked until: 2027-03-01T00:00:00Z", NOW).unwrap(),
            None,
            "beyond 30 days it keeps its old meaning rather than refusing"
        );
        assert_eq!(
            parse_block_wait("Waiting for the vendor.", NOW).unwrap(),
            None
        );
    }

    #[test]
    fn a_wait_named_wrongly_is_refused_with_the_format() {
        for (note, error) in [
            ("Waiting on: Spectrum", TaskWaitError::MissingNeeds),
            (
                "Waiting on: Spectrum\nNeeds: X4 back",
                TaskWaitError::OutsideNeedsRecheck,
            ),
            (
                "Waiting on: operator\nNeeds: approve\nRecheck: 2026-10-20T13:00:00Z",
                TaskWaitError::OperatorHasNoRecheck,
            ),
            (
                "Waiting on: Spectrum\nNeeds: X4 back\nRecheck: Tuesday",
                TaskWaitError::UnreadableTime("Tuesday".into()),
            ),
            (
                "Waiting on: Spectrum\nNeeds: X4 back\nRecheck: 2026-10-18T00:30:00Z",
                TaskWaitError::TimeOutOfRange,
            ),
            (
                "Recheck: 2026-10-20T13:00:00Z",
                TaskWaitError::RecheckWithoutParty,
            ),
        ] {
            let refused = parse_block_wait(note, NOW).unwrap_err();
            assert_eq!(refused, error, "{note}");
            assert!(refused.to_string().contains("Waiting on: operator"));
        }
    }

    fn view(kind: TaskWaitKind, cleared: bool) -> TaskWaitView {
        TaskWaitView {
            kind,
            needs: "approve".into(),
            party: None,
            resume_at: Some(NOW + 3_600),
            done_at: None,
            cleared,
        }
    }

    #[test]
    fn a_wait_clears_on_done_or_on_its_time() {
        let mut operator = view(TaskWaitKind::OperatorAction, false);
        operator.resume_at = None;
        assert!(!operator.cleared_at(NOW));
        operator.done_at = Some(NOW);
        assert!(operator.cleared_at(NOW));
        let outside = view(TaskWaitKind::Outside, false);
        assert!(!outside.cleared_at(NOW + 3_599));
        assert!(outside.cleared_at(NOW + 3_600));
    }

    #[test]
    fn a_typed_wait_names_its_owner_and_never_leaves_it_to_queen() {
        use crate::{NextMoveOwner, TaskState};
        let blocked = TaskState::Blocked;
        // What the earlier rules leave a gate-less block: Queen's to judge.
        let base = NextMoveOwner::Queen;
        assert_eq!(
            base.after_wait(
                blocked,
                Some(&view(TaskWaitKind::OperatorAction, false)),
                true,
                &[]
            ),
            NextMoveOwner::Operator
        );
        for kind in [TaskWaitKind::Outside, TaskWaitKind::Date] {
            assert_eq!(
                base.after_wait(blocked, Some(&view(kind, false)), true, &[]),
                NextMoveOwner::Blocked
            );
            assert_eq!(
                base.after_wait(blocked, Some(&view(kind, true)), true, &[]),
                NextMoveOwner::Worker
            );
            assert_eq!(
                base.after_wait(blocked, Some(&view(kind, true)), false, &[]),
                NextMoveOwner::Queen
            );
        }
        assert_eq!(
            base.after_wait(blocked, None, true, &[]),
            NextMoveOwner::Queen,
            "untyped stays Queen's"
        );
        assert_eq!(
            NextMoveOwner::Operator.after_wait(
                blocked,
                Some(&view(TaskWaitKind::Outside, false)),
                true,
                &[]
            ),
            NextMoveOwner::Operator,
            "a pending decision is still the next move"
        );
    }
}
