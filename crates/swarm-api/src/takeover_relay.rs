//! The takeover control channel: the target's screen out, the controller's
//! keystrokes in.
//!
//! ⚠️ THIS IS WHERE TAKEOVER DIFFERS FROM WATCHING, AND THE DIFFERENCE IS THE
//! INPUT PATH. Watching deliberately has none; ADR 0036 gates this one behind a
//! reasoned, exclusive, acknowledged, five-minute lease precisely because
//! typing into someone else's machine is a different act from looking at it.
//!
//! ⚠️ STILL NOTHING IS STORED. Both directions run through the same bounded
//! in-memory `FrameRelay` that watching uses — reused rather than copied, so
//! the "frames pass through and are never kept" property has one home. Keeper
//! is authority and relay and keeps no transcript, exactly as ADR 0036 says.

use std::sync::Arc;
use std::time::Duration;

use swarm_domain::{FederationStewardTakeoverLeaseId, FederationStewardTakeoverRelayRole};
use tokio::sync::broadcast;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::watch_relay::{
    FrameRelay, GrantStore, TAKEOVER_GRANT_PROTOCOL_PREFIX, grant_with_prefix,
};
use crate::{ApiError, AppState};

/// How often a live takeover socket re-reads the lease that authorizes it.
///
/// Shorter than the watch equivalent because more is at stake: this channel
/// carries INPUT, and a lease that has been reclaimed must stop carrying it
/// promptly rather than at the next convenient moment. The operator reclaiming
/// is the case this number exists for.
const RELAY_LIVENESS_CHECK_SECONDS: u64 = 5;

/// Both directions of one takeover, keyed by lease.
#[derive(Debug, Default)]
pub(crate) struct TakeoverRelay {
    /// The target's screen, travelling out to whoever holds the lease.
    screen: FrameRelay<FederationStewardTakeoverLeaseId>,
    /// The controller's keystrokes, travelling in to the target.
    ///
    /// ⚠️ A SEPARATE CHANNEL RATHER THAN A TAGGED ONE. Sharing a channel would
    /// mean a bug in tagging could deliver a keystroke to the watcher or a
    /// screen frame to the keyboard; separating them makes the wrong direction
    /// unrepresentable instead of merely unlikely.
    input: FrameRelay<FederationStewardTakeoverLeaseId>,
}

impl TakeoverRelay {
    /// Ends both directions of a takeover.
    pub(crate) fn retire(&self, lease: FederationStewardTakeoverLeaseId) {
        self.screen.retire(lease);
        self.input.retire(lease);
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct RelayRevision {
    /// The lease revision the caller believes is active.
    ///
    /// Carried so a stale client cannot attach to a lease that has since moved
    /// — it is the one place a revision fence is right, because attaching under
    /// a superseded lease is not the local operator being refused their own
    /// machine.
    revision: u64,
}

/// Tickets for a Keeper's browser opening a control channel.
pub(crate) type TakeoverGrantStore = GrantStore<FederationStewardTakeoverLeaseId>;

/// Keeper's own browser taking control of a Hive it holds.
///
/// ⚠️ SAME RELAY, SAME LEASE, DIFFERENT DOOR — exactly as watching works. A
/// browser cannot send an Authorization header on a WebSocket, so it offers a
/// single-use grant as a subprotocol; putting the operator token there would
/// leak a long-lived credential into proxy logs.
pub(crate) async fn apiary_takeover_control(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(lease_id): Path<FederationStewardTakeoverLeaseId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let grant = grant_with_prefix(&headers, TAKEOVER_GRANT_PROTOCOL_PREFIX).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "takeover_grant_required",
            "a short-lived takeover control grant is required",
        )
    })?;
    if !state.takeover_grants.consume(grant, lease_id) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_takeover_grant",
            "the takeover grant is invalid, expired, or already used",
        ));
    }
    // Re-checked after the grant is spent: the grant proves who asked, the
    // lease proves they still hold the Hive.
    if !lease_is_live(&state, lease_id, crate::unix_timestamp()) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "takeover_not_active",
            "that takeover is no longer active",
        ));
    }
    let permit = Arc::clone(&state.websocket_limit)
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "federation_websocket_limit_reached",
                "federation WebSocket capacity is exhausted",
            )
        })?;
    let controlled = Arc::clone(&state);
    let selected = format!("{TAKEOVER_GRANT_PROTOCOL_PREFIX}{grant}");
    Ok(websocket
        .protocols([selected])
        .on_upgrade(move |socket| async move {
            serve_takeover(
                socket,
                controlled,
                lease_id,
                FederationStewardTakeoverRelayRole::Source,
            )
            .await;
            drop(permit);
        }))
}

/// A member Hive attaching to a takeover it is party to.
///
/// The ROLE decides direction, and it is read from the lease rather than
/// claimed by the caller: the target publishes its screen and consumes input,
/// the source does the reverse. A caller that is neither is refused.
pub(crate) async fn federation_takeover_relay(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(lease_id): Path<FederationStewardTakeoverLeaseId>,
    Query(RelayRevision { revision }): Query<RelayRevision>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let credential = crate::federation_node_credential(&headers)?.to_owned();
    let now = crate::unix_timestamp();
    let authorization = crate::task_store(&state)?
        .authorize_federation_steward_takeover_relay(&credential, lease_id, revision, now)
        .map_err(|_| {
            ApiError::new(
                StatusCode::FORBIDDEN,
                "takeover_not_active",
                "no active takeover lease authorizes this relay",
            )
        })?;
    let permit = Arc::clone(&state.websocket_limit)
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "federation_websocket_limit_reached",
                "federation WebSocket capacity is exhausted",
            )
        })?;
    let role = authorization.role;
    let relayed = Arc::clone(&state);
    Ok(websocket.on_upgrade(move |socket| async move {
        serve_takeover(socket, relayed, lease_id, role).await;
        drop(permit);
    }))
}

/// Carries one side of a takeover until the lease stops authorizing it.
///
/// ⚠️ THE LEASE IS RE-READ, NOT TRUSTED FROM CONNECT TIME. Reclaim, release,
/// revocation and expiry all end a takeover, and the local operator reclaiming
/// must actually stop input arriving — a socket that kept carrying keystrokes
/// because it was authorized once is the failure ADR 0036's reclaim rule exists
/// to prevent.
async fn serve_takeover(
    mut socket: WebSocket,
    state: Arc<AppState>,
    lease: FederationStewardTakeoverLeaseId,
    role: FederationStewardTakeoverRelayRole,
) {
    let (outbound, inbound) = match role {
        // The target sends its screen and receives keystrokes.
        FederationStewardTakeoverRelayRole::Target => {
            (&state.takeover_relay.screen, &state.takeover_relay.input)
        }
        // The controller sends keystrokes and receives the screen.
        FederationStewardTakeoverRelayRole::Source => {
            (&state.takeover_relay.input, &state.takeover_relay.screen)
        }
    };
    let mut receiver: broadcast::Receiver<Vec<u8>> = inbound.subscribe(lease);
    let mut last_renewal: i64 = 0;
    let mut liveness = tokio::time::interval(Duration::from_secs(RELAY_LIVENESS_CHECK_SECONDS));
    liveness.tick().await;
    loop {
        tokio::select! {
            _ = liveness.tick() => {
                if !lease_is_live(&state, lease, crate::unix_timestamp()) {
                    state.takeover_relay.retire(lease);
                    return;
                }
            }
            frame = receiver.recv() => {
                match frame {
                    Ok(frame) => {
                        if socket.send(Message::Binary(frame.into())).await.is_err() {
                            return;
                        }
                    }
                    // A terminal cannot be resynchronised from a gap, and a
                    // dropped keystroke must not be replayed late into someone
                    // else's shell. Closing sends both sides back for a fresh
                    // snapshot, which is the only honest answer.
                    Err(broadcast::error::RecvError::Lagged(_) | broadcast::error::RecvError::Closed) => return,
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Binary(frame))) => {
                        if role == FederationStewardTakeoverRelayRole::Source {
                            renew_on_input(&state, lease, &mut last_renewal);
                        }
                        outbound.publish(lease, frame.to_vec());
                    }
                    // Binary only. Text would be a second protocol inside this
                    // one, and this channel carries terminal bytes.
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return,
                }
            }
        }
    }
}

/// How close to expiry a lease in use must be before input renews it.
const RENEW_WHEN_REMAINING_SECONDS: i64 = 120;
/// The fewest seconds between two renewals of one lease.
const MIN_SECONDS_BETWEEN_RENEWALS: i64 = 30;

/// Extends a Keeper-held takeover that is being actively typed into.
///
/// ⚠️ NOTHING DID THIS, SO EVERY TAKEOVER ENDED FIVE MINUTES AFTER IT STARTED,
/// MID-KEYSTROKE. The lease is five minutes long by design (ADR 0036), and the
/// design is that use keeps it alive — `queue_federation_steward_takeover_renewal`
/// even says renewal happens "after authenticated input". Nothing called it,
/// and nothing renewed a Keeper's own lease either, so a Keeper halfway through
/// an incident would find the window dead with "The takeover ended." Nobody had
/// hit it only because every test so far was shorter than five minutes.
///
/// Renewal is bounded twice: only when the lease is within two minutes of
/// lapsing, and at most once per thirty seconds, so typing does not become a
/// database write per keystroke. The doorbell is rung so the held Hive refreshes
/// its own copy before that copy lapses and releases automation underneath a
/// Keeper who is still typing.
///
/// A Steward-held lease is renewed here too. Its keystrokes arrive at the
/// Keeper through the federation relay, authorised by the Steward Hive's node
/// credential as this lease's source — the same proof a journalled renewal
/// would carry — so the lease is extended where it lives.
fn renew_on_input(state: &AppState, lease: FederationStewardTakeoverLeaseId, last: &mut i64) {
    let now = crate::unix_timestamp();
    if now - *last < MIN_SECONDS_BETWEEN_RENEWALS {
        return;
    }
    let Ok(store) = crate::task_store(state) else {
        return;
    };
    let expires_at = store.apiary_takeover_audit(200).ok().and_then(|audit| {
        audit
            .into_iter()
            .find(|entry| entry.lease.id == lease)
            .map(|entry| entry.lease.expires_at)
    });
    if !expires_at.is_some_and(|expires_at| should_renew(now, *last, expires_at)) {
        return;
    }
    *last = now;
    // Keeper-held and Steward-held alike: both reach this through a connection
    // already authorised as the lease's source.
    if store
        .extend_takeover_on_source_input(lease, now)
        .unwrap_or(false)
    {
        crate::announce_federation_change(state, swarm_domain::FederationChangeKind::Unspecified);
    }
}

/// Whether input at `now` should extend a lease that lapses at `expires_at`.
///
/// Pure so the two bounds can be pinned without a relay: near expiry only, and
/// never twice inside the spacing window.
fn should_renew(now: i64, last_renewal: i64, expires_at: i64) -> bool {
    now - last_renewal >= MIN_SECONDS_BETWEEN_RENEWALS
        && expires_at - now <= RENEW_WHEN_REMAINING_SECONDS
        && expires_at > now
}

/// Whether an active, unexpired lease still names this takeover.
fn lease_is_live(state: &AppState, lease: FederationStewardTakeoverLeaseId, now: i64) -> bool {
    let Ok(store) = crate::task_store(state) else {
        return false;
    };
    let live = |leases: &[swarm_domain::FederationStewardTakeoverLease]| {
        leases.iter().any(|held| {
            held.id == lease
                && held.state == swarm_domain::FederationStewardTakeoverState::Active
                && held.expires_at > now
        })
    };
    // ⚠️ BOTH TABLES, BECAUSE THE TWO ROLES HOLD THE LEASE IN DIFFERENT PLACES.
    // A member has only its local projection; Keeper has only the Apiary table
    // and no projection of its own. Reading one would have silently dropped
    // every Keeper-side control channel five seconds after it opened.
    if store
        .federation_steward_takeover_local_state()
        .is_ok_and(|local| live(&local.leases))
    {
        return true;
    }
    store.apiary_takeover_audit(200).is_ok_and(|audit| {
        live(
            &audit
                .into_iter()
                .map(|entry| entry.lease)
                .collect::<Vec<_>>(),
        )
    })
}

#[cfg(test)]
mod renewal_tests {
    use super::should_renew;

    /// ⚠️ A takeover being typed into must outlive five minutes. Nothing renewed
    /// one, so every takeover died at the five-minute mark, mid-keystroke.
    #[test]
    fn input_near_expiry_renews() {
        assert!(should_renew(1_000, 0, 1_060));
    }

    /// Typing is not a write per keystroke: far from expiry, nothing happens.
    #[test]
    fn input_far_from_expiry_does_not_renew() {
        assert!(!should_renew(1_000, 0, 1_290));
    }

    /// And near expiry, not more than once per spacing window.
    #[test]
    fn renewals_are_spaced() {
        assert!(!should_renew(1_000, 990, 1_060));
        assert!(should_renew(1_000, 970, 1_060));
    }

    /// A lease that has already lapsed is over. Input does not resurrect it —
    /// that would undo a reclaim or an expiry the held Hive has already acted on.
    #[test]
    fn a_lapsed_lease_is_not_revived_by_input() {
        assert!(!should_renew(1_000, 0, 999));
        assert!(!should_renew(1_000, 0, 1_000));
    }
}
