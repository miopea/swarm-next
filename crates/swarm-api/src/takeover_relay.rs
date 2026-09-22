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

use crate::watch_relay::FrameRelay;
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
                    Some(Ok(Message::Binary(frame))) => outbound.publish(lease, frame.to_vec()),
                    // Binary only. Text would be a second protocol inside this
                    // one, and this channel carries terminal bytes.
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return,
                }
            }
        }
    }
}

/// Whether an active, unexpired lease still names this takeover.
fn lease_is_live(state: &AppState, lease: FederationStewardTakeoverLeaseId, now: i64) -> bool {
    crate::task_store(state).is_ok_and(|store| {
        store
            .federation_steward_takeover_local_state()
            .is_ok_and(|local| {
                local.leases.iter().any(|held| {
                    held.id == lease
                        && held.state == swarm_domain::FederationStewardTakeoverState::Active
                        && held.expires_at > now
                })
            })
    })
}
