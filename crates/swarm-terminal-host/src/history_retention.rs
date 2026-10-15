//! The host's side of terminal history retention (ADR 0118): it applies what
//! the Hive asks for while it runs, and says what it is using.

use std::path::Path;

use swarm_terminal::{
    HistoryStore, history_retention_applied_path, history_retention_request_path,
    read_history_retention, write_history_retention,
};
use tracing::{info, warn};

/// How often the host looks for a new retention. Latency only: whether a
/// setting is in effect is decided by what the host writes back, never by
/// assuming a minute has passed.
pub const RETENTION_CHECK_SECONDS: u64 = 60;

/// Applies the retention the Hive asks for, if it differs from the one in use,
/// and records the one in use beside the history. Returns it.
///
/// A request that is missing or unreadable changes nothing: the host keeps
/// what it started with, and says so.
pub fn apply_requested_history_retention(history: &HistoryStore, history_dir: &Path) -> u64 {
    if let Some(requested) = read_history_retention(&history_retention_request_path(history_dir))
        && requested != history.max_age_seconds()
    {
        match history.set_max_age(requested) {
            Ok(()) => info!(
                max_age_seconds = requested,
                "terminal history retention changed"
            ),
            Err(error) => warn!(%error, "terminal history retention could not change"),
        }
    }
    let in_use = history.max_age_seconds();
    let applied = history_retention_applied_path(history_dir);
    if read_history_retention(&applied) != Some(in_use)
        && let Err(error) = write_history_retention(&applied, in_use)
    {
        warn!(%error, "could not record the terminal history retention in use");
    }
    in_use
}

#[cfg(test)]
mod tests {
    use swarm_terminal::HistoryLimits;

    use super::*;

    /// ⚠️ ADR 0118: the host takes a new retention while it runs and says
    /// what it is using, which is how the Hive knows it is in effect.
    #[test]
    fn the_host_applies_a_requested_retention_and_says_so() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("terminal-history");
        let history = HistoryStore::open(&dir, HistoryLimits::default()).unwrap();
        let applied = history_retention_applied_path(&dir);

        // Nothing asked: it keeps its own, and says which.
        assert_eq!(
            apply_requested_history_retention(&history, &dir),
            7 * 86_400
        );
        assert_eq!(read_history_retention(&applied), Some(7 * 86_400));

        write_history_retention(&history_retention_request_path(&dir), 86_400).unwrap();
        assert_eq!(apply_requested_history_retention(&history, &dir), 86_400);
        assert_eq!(history.max_age_seconds(), 86_400);
        assert_eq!(read_history_retention(&applied), Some(86_400));

        // A request it cannot read changes nothing.
        std::fs::write(history_retention_request_path(&dir), "garbage").unwrap();
        assert_eq!(apply_requested_history_retention(&history, &dir), 86_400);
    }
}
