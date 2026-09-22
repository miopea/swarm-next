//! Where a cross-Hive dependency stands, including when it can never be met.
//!
//! ⚠️ THE ORPHANED CASE IS THE REASON THIS MODULE EXISTS. A task waiting on
//! work nobody will ever do is not blocked — it is stranded, and the difference
//! matters because blocked work is invisible by design. Measured on this Hive
//! on 2026-09-20: two tasks whose state had stopped matching reality were the
//! only live recovery obligations and forced EVERY Queen run to Incomplete for
//! hours. Cross-Hive links add a better-hidden way to manufacture that, so the
//! unblock is built before anything can strand.

use serde::{Deserialize, Serialize};

use crate::{ApiaryTaskId, HiveId};

/// Why a dependency can never be satisfied.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrphanCause {
    /// The upstream task was abandoned. Somebody decided it will not happen.
    UpstreamAbandoned,
    /// The Hive that owned the upstream task has left the Apiary.
    ///
    /// ⚠️ NOT THE SAME AS ABANDONED, and worth keeping apart in the record. An
    /// abandoned task was a decision somebody made; a departed Hive is work
    /// that simply walked out of the Apiary still open, and the right follow-up
    /// differs — one is closed, the other may need re-homing.
    HomeHiveDeparted,
}

/// What one dependency edge means right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "standing")]
pub enum PrerequisiteStanding {
    /// The upstream is still open and somebody can still do it.
    Waiting,
    /// The upstream finished. Ordinary unblocking.
    Satisfied,
    /// The upstream will never finish.
    ///
    /// ⚠️ THIS UNBLOCKS RATHER THAN WAITS, which is the operator's decision and
    /// deliberately not the conservative choice. Holding the dependent forever
    /// would be safe for the ordering and disastrous for the board: it
    /// manufactures work that is waiting on nothing, which nobody sees until
    /// something else forces them to look.
    Orphaned { cause: OrphanCause },
}

impl PrerequisiteStanding {
    /// Whether this edge still holds the dependent back.
    #[must_use]
    pub fn blocks(self) -> bool {
        matches!(self, Self::Waiting)
    }

    /// Whether this edge is owed an explanation to Queen.
    #[must_use]
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::Orphaned { .. })
    }
}

/// One dependency, with the sentence that explains why it exists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryPrerequisiteStatus {
    pub task_id: ApiaryTaskId,
    pub prerequisite_id: ApiaryTaskId,
    /// Why the ordering was recorded. Carried through so an unblock can say
    /// what stopped being true rather than only that something did.
    pub reason: String,
    /// The Hive that owned the upstream, when it had one.
    pub upstream_home_hive_id: Option<HiveId>,
    pub standing: PrerequisiteStanding,
}

/// Whether a task with these edges is still held back.
///
/// ⚠️ ORPHANED EDGES DO NOT COUNT, and a task blocked ONLY by orphans is free.
/// That is the whole behaviour this task was written for.
#[must_use]
pub fn blocked_by(edges: &[ApiaryPrerequisiteStatus]) -> bool {
    edges.iter().any(|edge| edge.standing.blocks())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(standing: PrerequisiteStanding) -> ApiaryPrerequisiteStatus {
        ApiaryPrerequisiteStatus {
            task_id: ApiaryTaskId::new(),
            prerequisite_id: ApiaryTaskId::new(),
            reason: "The API has to land first.".to_owned(),
            upstream_home_hive_id: None,
            standing,
        }
    }

    #[test]
    fn a_task_waiting_on_live_work_is_blocked() {
        assert!(blocked_by(&[edge(PrerequisiteStanding::Waiting)]));
    }

    #[test]
    fn a_task_whose_upstream_finished_is_free() {
        assert!(!blocked_by(&[edge(PrerequisiteStanding::Satisfied)]));
    }

    /// ⚠️ THE POINT OF THE WHOLE MODULE. Work waiting on something nobody will
    /// ever do is stranded, not blocked, and blocked work is invisible by
    /// design.
    #[test]
    fn a_task_held_only_by_orphans_is_free_and_owed_an_explanation() {
        for cause in [
            OrphanCause::UpstreamAbandoned,
            OrphanCause::HomeHiveDeparted,
        ] {
            let orphan = edge(PrerequisiteStanding::Orphaned { cause });
            assert!(
                !blocked_by(std::slice::from_ref(&orphan)),
                "{cause:?} does not hold it"
            );
            assert!(orphan.standing.needs_attention(), "{cause:?} is explained");
        }
    }

    /// ⚠️ AND ONE LIVE EDGE STILL HOLDS IT. Unblocking on the strength of some
    /// other dependency being dead would reorder work that genuinely has an
    /// order, which is the opposite failure and just as real.
    #[test]
    fn one_live_prerequisite_still_blocks_however_many_are_orphaned() {
        let edges = vec![
            edge(PrerequisiteStanding::Orphaned {
                cause: OrphanCause::HomeHiveDeparted,
            }),
            edge(PrerequisiteStanding::Orphaned {
                cause: OrphanCause::UpstreamAbandoned,
            }),
            edge(PrerequisiteStanding::Waiting),
        ];
        assert!(blocked_by(&edges));
    }

    #[test]
    fn a_task_with_no_prerequisites_is_not_blocked_by_this() {
        assert!(!blocked_by(&[]));
    }
}
