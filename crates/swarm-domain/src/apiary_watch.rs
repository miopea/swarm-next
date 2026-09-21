//! Watching a Hive: one live window, the same depth for everyone who may open
//! it, never stored, and never invisible to the person being watched.
//!
//! ⚠️ TWO PROPERTIES MAKE THIS SAFE ENOUGH TO EXIST, and without both it is
//! standing surveillance of every member's machine:
//!
//! 1. **Live only, never stored.** Keeper relays frames and persists none of
//!    them. What IS recorded is the SESSION — who watched which Hive, and when.
//!    That distinction is the whole design: a compromised Keeper then exposes
//!    what is on one screen right now, rather than months of every member's
//!    terminal history including whatever they have typed.
//! 2. **Always visible while it lasts.** The watched operator sees the watch
//!    for as long as it is open. Invisible observation is the property most
//!    likely to make people distrust software they run on their own machine,
//!    and it would be discovered rather than disclosed.
//!
//! This module is the control plane for both. There is deliberately no frame
//! type here.

use serde::{Deserialize, Serialize};

use crate::{HiveId, OperatorId, StewardCapability, Stewardship, StewardshipId};

/// How long one watch runs before it must be renewed.
///
/// ⚠️ A WATCH MUST END BY ITSELF. An unbounded watch is the standing
/// surveillance this design exists to avoid — a window someone forgot to close
/// is indistinguishable from one left open on purpose. It renews only while the
/// watcher's window is genuinely open, so walking away closes it.
///
/// Five minutes matches the takeover lease in ADR 0036 rather than inventing a
/// second number: two durations would be two things to explain and one of them
/// would eventually be wrong.
pub const WATCH_LEASE_SECONDS: i64 = 300;

/// Why someone is allowed to watch a Hive.
///
/// ⚠️ THIS ANSWERS "WHICH HIVES", AND NOTHING ANSWERS "HOW MUCH". Depth and
/// breadth are different things: a grant decides which Hives you may open a
/// window into, never how much of one you see. There is no depth field here, on
/// any variant, and there is no parameter for one anywhere in this module —
/// that is the enforcement, not a convention. A Steward sees a Hive in scope
/// exactly as Keeper would.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchAuthority {
    /// The Apiary's Keeper, who may watch any Hive in it.
    Keeper,
    /// A Steward, for a Hive inside their granted scope.
    Steward(StewardshipId),
}

/// Where a watch is in its life.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    /// Authorized at Keeper, not yet acknowledged by the target Hive.
    ///
    /// ⚠️ NO FRAME MAY FLOW IN THIS STATE. The target acknowledging is what
    /// proves it knows it is being watched, and the visibility promise is only
    /// kept if the Hive has said so before anything is relayed.
    Requested,
    /// Acknowledged by the target and live.
    Active,
    /// Over. Kept as the record of who looked and when.
    Ended,
}

impl WatchState {
    /// Whether this watch still occupies the target's notice.
    #[must_use]
    pub fn is_open(self) -> bool {
        matches!(self, Self::Requested | Self::Active)
    }

    /// Whether frames may be relayed right now.
    #[must_use]
    pub fn relays(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// One watch, as both sides hold it.
///
/// ⚠️ THERE IS NO `reason` FIELD, AND ITS ABSENCE IS A DECISION. The operator
/// was offered "who and why" and chose "who, no reason required", to keep a
/// quick glance low-friction. This is deliberately asymmetric with ADR 0036,
/// which requires and audits a reason for takeover. The consequence is that this
/// record can answer "who looked, and when" and can NEVER answer "why". If that
/// question ever matters after the fact, this is what made it unanswerable — so
/// do not quietly add a required reason later without saying that it reverses
/// the operator's call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryWatch {
    pub id: crate::ApiaryWatchId,
    pub apiary_id: crate::ApiaryId,
    /// Who is watching. Shown to the watched operator by name while it is open.
    pub watcher_operator_id: OperatorId,
    pub target_hive_id: HiveId,
    pub authority: WatchAuthority,
    pub state: WatchState,
    pub requested_at: i64,
    pub acknowledged_at: Option<i64>,
    /// When this watch lapses unless renewed.
    pub expires_at: i64,
    pub ended_at: Option<i64>,
}

impl ApiaryWatch {
    /// Whether this watch is live and unexpired at `now`.
    ///
    /// Expiry is evaluated rather than trusted: a lease whose row still says
    /// `active` because nothing has swept it yet is not a live watch, and the
    /// target must stop showing it at the same moment the relay stops honouring
    /// it. One function so the two cannot disagree.
    #[must_use]
    pub fn is_live(&self, now: i64) -> bool {
        self.state.relays() && now < self.expires_at
    }

    /// Whether the target should still be TELLING its operator about this.
    ///
    /// ⚠️ WIDER THAN `is_live` ON PURPOSE. A requested-but-unacknowledged watch
    /// is shown too, so the notice never lags the authorization — the operator
    /// learns someone is opening a window before the window opens, not after.
    #[must_use]
    pub fn is_visible(&self, now: i64) -> bool {
        self.state.is_open() && now < self.expires_at
    }
}

/// Whether this operator may watch this Hive, and on what authority.
///
/// `None` means no. Breadth only — see [`WatchAuthority`].
#[must_use]
pub fn watch_authority(
    viewer: OperatorId,
    keeper_operator_id: OperatorId,
    target_hive_id: HiveId,
    stewardships: &[Stewardship],
) -> Option<WatchAuthority> {
    if viewer == keeper_operator_id {
        return Some(WatchAuthority::Keeper);
    }
    stewardships
        .iter()
        .find(|scope| {
            scope.steward_operator_id == viewer
                && scope.capabilities.contains(&StewardCapability::Observe)
                && scope.managed_hive_ids.contains(&target_hive_id)
        })
        .map(|scope| WatchAuthority::Steward(scope.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiaryId;

    fn scope(
        steward: OperatorId,
        hives: Vec<HiveId>,
        capabilities: Vec<StewardCapability>,
    ) -> Stewardship {
        Stewardship {
            id: StewardshipId::new(),
            apiary_id: ApiaryId::new(),
            steward_operator_id: steward,
            managed_hive_ids: hives,
            capabilities,
        }
    }

    #[test]
    fn the_keeper_may_watch_any_hive_without_a_stewardship() {
        let keeper = OperatorId::new();
        assert_eq!(
            watch_authority(keeper, keeper, HiveId::new(), &[]),
            Some(WatchAuthority::Keeper)
        );
    }

    /// ⚠️ THE OPERATOR'S OWN FRAMING: a Steward over Paul's Hive "would have
    /// line of site" into it, and into nothing else.
    #[test]
    fn a_steward_sees_the_hives_in_scope_and_no_others() {
        let steward = OperatorId::new();
        let keeper = OperatorId::new();
        let mine = HiveId::new();
        let theirs = HiveId::new();
        let scopes = vec![scope(
            steward,
            vec![mine],
            vec![StewardCapability::Observe, StewardCapability::Assist],
        )];
        assert_eq!(
            watch_authority(steward, keeper, mine, &scopes),
            Some(WatchAuthority::Steward(scopes[0].id))
        );
        assert_eq!(
            watch_authority(steward, keeper, theirs, &scopes),
            None,
            "a grant is not transitive across the Apiary"
        );
    }

    #[test]
    fn a_stewardship_without_observe_grants_no_window() {
        let steward = OperatorId::new();
        let hive = HiveId::new();
        let scopes = vec![scope(
            steward,
            vec![hive],
            vec![StewardCapability::Assign, StewardCapability::ManageProjects],
        )];
        assert_eq!(
            watch_authority(steward, OperatorId::new(), hive, &scopes),
            None
        );
    }

    #[test]
    fn an_unrelated_member_may_not_watch_anyone() {
        let hive = HiveId::new();
        let scopes = vec![scope(
            OperatorId::new(),
            vec![hive],
            vec![StewardCapability::Observe],
        )];
        assert_eq!(
            watch_authority(OperatorId::new(), OperatorId::new(), hive, &scopes),
            None
        );
    }

    fn watch(state: WatchState, expires_at: i64) -> ApiaryWatch {
        ApiaryWatch {
            id: crate::ApiaryWatchId::new(),
            apiary_id: ApiaryId::new(),
            watcher_operator_id: OperatorId::new(),
            target_hive_id: HiveId::new(),
            authority: WatchAuthority::Keeper,
            state,
            requested_at: 0,
            acknowledged_at: Some(1),
            expires_at,
            ended_at: None,
        }
    }

    /// ⚠️ THE NOTICE MUST NOT LAG THE AUTHORIZATION. The operator learns a
    /// window is being opened before it opens, rather than finding out once
    /// someone is already looking.
    #[test]
    fn a_requested_watch_is_already_visible_but_relays_nothing() {
        let requested = watch(WatchState::Requested, 100);
        assert!(requested.is_visible(10), "shown before it is acknowledged");
        assert!(!requested.is_live(10), "and nothing flows until it is");
    }

    /// An expired lease is not a live watch even while its row still says
    /// active, and it stops being SHOWN at the same instant it stops being
    /// honoured — one function, so the two cannot drift apart.
    #[test]
    fn an_expired_watch_neither_relays_nor_shows() {
        let stale = watch(WatchState::Active, 100);
        assert!(stale.is_live(99));
        assert!(stale.is_visible(99));
        assert!(!stale.is_live(100), "a lapsed lease relays nothing");
        assert!(!stale.is_visible(100), "and claims nothing on screen");
    }

    #[test]
    fn an_ended_watch_is_kept_as_the_record_but_shows_nothing() {
        let mut ended = watch(WatchState::Ended, i64::MAX);
        ended.ended_at = Some(50);
        assert!(!ended.is_visible(60));
        assert!(!ended.is_live(60));
    }
}
