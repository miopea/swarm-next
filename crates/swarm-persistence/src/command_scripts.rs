//! The copy of a command card's script that Swarm keeps for whoever answers
//! the card (ADR 0187).
//!
//! ⚠️ THE APPROVER SAW A PATH AND A HASH. In the 14 days to 2026-11-22 the
//! operator answered 288 cards that ran a script and Queen 371, and neither
//! was shown the script: the card named a file, mostly in a worker's
//! scratchpad, and the sha256 Swarm took of it. Swarm now keeps the text it
//! hashed, from the same read, so the text shown is the text that hash pins.

use rusqlite::OptionalExtension as _;
use serde::Serialize;
use swarm_domain::{DecisionRequestId, RequestedScript};

use crate::{TaskStore, TaskStoreError};

/// The most script text Swarm keeps for one card. A longer script keeps its
/// path and hash, and its card says why no text was kept.
pub const MAX_SCRIPT_TEXT_BYTES: usize = 64 * 1024;
/// The longest reason a card gives for keeping no text.
pub const MAX_SCRIPT_WITHHELD_BYTES: usize = 512;

/// What Swarm kept of a card's script when it was filed: its text, or why it
/// kept none. Both are absent on a card filed before Swarm kept scripts.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ScriptSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withheld: Option<String>,
}

impl ScriptSnapshot {
    /// Text, or a reason, and never both: a card shows one or the other.
    pub(crate) fn is_valid(&self) -> bool {
        match (&self.text, &self.withheld) {
            (Some(text), None) => text.len() <= MAX_SCRIPT_TEXT_BYTES,
            (None, Some(reason)) => {
                !reason.trim().is_empty() && reason.len() <= MAX_SCRIPT_WITHHELD_BYTES
            }
            _ => false,
        }
    }
}

/// A script named when a card is filed, with what Swarm kept of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiledScript {
    pub script: RequestedScript,
    pub snapshot: ScriptSnapshot,
}

impl TaskStore {
    /// The script a card names and what Swarm kept of it. `None` when the card
    /// names no script.
    ///
    /// # Errors
    /// `DecisionNotFound` for an unknown card, or a store error.
    pub fn decision_script(
        &self,
        id: DecisionRequestId,
    ) -> Result<Option<FiledScript>, TaskStoreError> {
        type Row = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let connection = self.connection()?;
        let row: Option<Row> = connection
            .query_row(
                "SELECT requested_script_path, requested_script_sha256,
                        requested_script_text, requested_script_withheld
                 FROM decision_requests WHERE id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (path, sha256, text, withheld) = row.ok_or(TaskStoreError::DecisionNotFound)?;
        Ok(path.zip(sha256).map(|(path, sha256)| FiledScript {
            script: RequestedScript { path, sha256 },
            snapshot: ScriptSnapshot { text, withheld },
        }))
    }
}

/// Two nullable columns on the card: the text kept, or why none was. Cards
/// filed earlier keep neither, and say so when read.
pub(crate) fn migrate_command_script_snapshots(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    crate::add_column_once(
        transaction,
        "decision_requests",
        "requested_script_text",
        "TEXT",
        version,
    )?;
    crate::add_column_once(
        transaction,
        "decision_requests",
        "requested_script_withheld",
        "TEXT",
        version,
    )
}
