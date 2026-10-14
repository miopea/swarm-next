//! The worker model (ADR 0116): the one model every Claude repository worker on
//! a Hive starts with. Queen keeps her own default and is never moved by it.
//!
//! These are the rules only. The API writes the chosen model beside a worker's
//! config before each start, the terminal host relays it as `--model`, and a
//! running session keeps the model it started with until it next starts.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The model a Hive's Claude repository workers start with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerModel {
    /// Swarm names no model and Claude Code uses its own. Every Hive's
    /// starting point, and what every Hive did before this setting existed.
    #[default]
    Default,
    Opus,
    Sonnet,
    Haiku,
}

impl WorkerModel {
    pub const ALL: [Self; 4] = [Self::Default, Self::Opus, Self::Sonnet, Self::Haiku];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }

    /// The name passed to Claude Code, or `None` when Swarm passes none.
    ///
    /// Claude Code's own aliases rather than model ids, so the setting follows
    /// its current mapping instead of pinning a release that will be retired.
    #[must_use]
    pub const fn alias(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Opus => Some("opus"),
            Self::Sonnet => Some("sonnet"),
            Self::Haiku => Some("haiku"),
        }
    }
}

impl fmt::Display for WorkerModel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for WorkerModel {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|model| model.as_str() == value)
            .ok_or_else(|| format!("unknown worker model {value:?}"))
    }
}

/// What one start does to the `model` in a repository worker's settings file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchModelChange {
    /// Write this model. The Hive names one, and it is one model for every
    /// worker, so it replaces whatever the file held (the operator's ruling,
    /// decision 01a13bc9: "No, one model for all").
    Write(&'static str),
    /// Remove the model Swarm wrote there earlier: the Hive is back on Claude's
    /// own default.
    Remove,
    /// Leave the file as it is: it already names the Hive's model, or the Hive
    /// names none and the model there is not Swarm's.
    Leave,
}

/// What a repository worker's start does to its settings file.
///
/// `in_file` is the model the file names now, and `swarm_wrote` the one Swarm
/// last wrote there, if it still stands.
///
/// ⚠️ BACK TO DEFAULT REMOVES ONLY WHAT SWARM WROTE. A model somebody put in the
/// file by hand predates this setting and is not Swarm's to delete; the Hive
/// passing no model leaves it exactly as it was before the setting existed.
#[must_use]
pub fn launch_model_change(
    hive: WorkerModel,
    in_file: Option<&str>,
    swarm_wrote: Option<&str>,
) -> LaunchModelChange {
    match hive.alias() {
        Some(model) if in_file == Some(model) => LaunchModelChange::Leave,
        Some(model) => LaunchModelChange::Write(model),
        None if in_file.is_some() && in_file == swarm_wrote => LaunchModelChange::Remove,
        None => LaunchModelChange::Leave,
    }
}

/// Whether a session started with `launched_with` is still short of the Hive's
/// model. A Hive on Claude's own default asks nothing of anyone. A session with
/// no recorded launch model started before this setting existed, so it is
/// short of any model the Hive names, not assumed to be on it.
#[must_use]
pub fn session_short_of_model(hive: WorkerModel, launched_with: Option<&str>) -> bool {
    hive.alias()
        .is_some_and(|model| launched_with != Some(model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_default_names_no_model() {
        for model in WorkerModel::ALL {
            assert_eq!(model.as_str().parse::<WorkerModel>(), Ok(model));
        }
        assert!("gpt".parse::<WorkerModel>().is_err());
        assert_eq!(WorkerModel::default(), WorkerModel::Default);
        assert_eq!(WorkerModel::Default.alias(), None);
        assert_eq!(WorkerModel::Sonnet.alias(), Some("sonnet"));
        assert_eq!(
            serde_json::to_value(WorkerModel::Haiku).unwrap(),
            serde_json::json!("haiku")
        );
    }

    /// One model for all: a named model replaces whatever the file held,
    /// including a hand edit, because the operator ruled out exceptions.
    #[test]
    fn a_named_model_is_written_over_anything() {
        for (in_file, wrote) in [
            (None, None),
            (Some("opus"), None),
            (Some("haiku"), Some("haiku")),
        ] {
            assert_eq!(
                launch_model_change(WorkerModel::Sonnet, in_file, wrote),
                LaunchModelChange::Write("sonnet")
            );
        }
        // Already there, whoever put it there: nothing to write, and nothing
        // for Swarm to claim as its own.
        assert_eq!(
            launch_model_change(WorkerModel::Sonnet, Some("sonnet"), None),
            LaunchModelChange::Leave
        );
    }

    /// ⚠️ Back to default takes back only Swarm's own value.
    #[test]
    fn back_to_default_removes_only_what_swarm_wrote() {
        assert_eq!(
            launch_model_change(WorkerModel::Default, Some("sonnet"), Some("sonnet")),
            LaunchModelChange::Remove
        );
        assert_eq!(
            launch_model_change(WorkerModel::Default, Some("opus"), None),
            LaunchModelChange::Leave,
            "a hand edit that predates the setting is not Swarm's to delete"
        );
        assert_eq!(
            launch_model_change(WorkerModel::Default, Some("opus"), Some("sonnet")),
            LaunchModelChange::Leave,
            "somebody changed it since Swarm wrote it"
        );
        assert_eq!(
            launch_model_change(WorkerModel::Default, None, Some("sonnet")),
            LaunchModelChange::Leave
        );
    }

    #[test]
    fn a_session_is_short_until_it_starts_on_the_hives_model() {
        assert!(!session_short_of_model(WorkerModel::Default, None));
        assert!(!session_short_of_model(WorkerModel::Default, Some("opus")));
        assert!(!session_short_of_model(WorkerModel::Sonnet, Some("sonnet")));
        assert!(session_short_of_model(WorkerModel::Sonnet, Some("opus")));
        assert!(
            session_short_of_model(WorkerModel::Sonnet, None),
            "a session from before the setting is not assumed to comply"
        );
    }
}
