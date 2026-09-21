//! What a Keeper announces to a connected member.
//!
//! ⚠️ THIS IS A DOORBELL, NOT A PIPE, AND THE TYPE IS SHAPED TO KEEP IT ONE.
//! A notice says that something changed and names WHICH thing, so the member
//! can fetch the right feed. It carries no revision, no cursor, no payload and
//! no command.
//!
//! The omissions are deliberate. A revision number here would be acted on by
//! somebody eventually, and then two paths would carry state and could disagree
//! — which is the whole failure this shape avoids. Durable state keeps arriving
//! through the existing polled, cursored, fail-closed feed; losing a notice
//! costs latency and nothing else.
//!
//! Doc 98's surviving constraint says the same thing in product terms: the
//! connection "should announce durable changes, not carry arbitrary remote
//! commands or establish authority". Authority stays with the grant, rechecked
//! at Keeper on every authenticated request.

use serde::{Deserialize, Serialize};

use crate::ApiaryId;

/// Which durable feed moved. Naming it lets a member fetch one thing instead of
/// everything; it is routing information, not state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationChangeKind {
    /// The ordered shared-task feed.
    Tasks,
    /// The signed promoted-project catalog.
    Catalog,
    /// The Apiary policy body.
    Policy,
    /// The public member directory.
    Directory,
    /// Someone started or stopped watching a Hive.
    ///
    /// ⚠️ THIS ONE IS URGENT IN A WAY THE OTHERS ARE NOT. A stale task feed is
    /// inconvenient; a watched operator who has not been told yet is the state
    /// this feature is forbidden to be in. The pacing gate holds an ordinary
    /// pass to 60 seconds, which is both too slow to open a window through and
    /// too long to leave someone uninformed, so a watch rings the doorbell.
    Watch,
    /// Something changed and Keeper could not say what.
    ///
    /// ⚠️ NOT AN ERROR, AND THE REASON IT EXISTS IS BACKPRESSURE. A member whose
    /// receiver lagged behind the broadcast has provably missed notices, and the
    /// honest recovery for a doorbell is to ring it once for everything rather
    /// than guess which ones were lost. Reconciling more than necessary costs a
    /// poll; reconciling less loses a change.
    Unspecified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationChangeNotice {
    pub kind: FederationChangeKind,
    pub apiary_id: ApiaryId,
    /// Keeper's clock when it announced. For display and debugging only —
    /// nothing may order or deduplicate on it, because the feed's own cursor is
    /// what makes fetching idempotent.
    pub announced_at: i64,
}

impl FederationChangeNotice {
    #[must_use]
    pub fn new(kind: FederationChangeKind, apiary_id: ApiaryId, announced_at: i64) -> Self {
        Self {
            kind,
            apiary_id,
            announced_at,
        }
    }
}
