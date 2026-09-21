//! The outbound member-to-Keeper event socket.
//!
//! One connection per Hive, originating at the member, so members still need no
//! inbound address. Keeper announces that a durable feed moved; the member's
//! existing reconciliation owner fetches and applies it.
//!
//! ⚠️ THE SOCKET IS A DOORBELL AND MUST STAY ONE. It carries no state and no
//! commands. Losing it costs latency, not correctness, because every durable
//! change still arrives through the polled, cursored, fail-closed feed. That
//! asymmetry is the reason this design was chosen over moving state onto the
//! stream: a dropped doorbell means state arrives slightly later, a dropped
//! pipe means state stops.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::{Message as ClientMessage, client::IntoClientRequest};

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use swarm_domain::{FederationChangeKind, FederationChangeNotice};
use tokio::sync::broadcast;

use crate::{ApiError, AppState, apiary_service, federation_node_credential, unix_timestamp};

/// How many notices a slow member may fall behind before it is told to
/// reconcile everything instead.
///
/// Small on purpose. A backlog of doorbell rings has no value — ringing twice
/// means the same thing as ringing once — so buffering deeply would only delay
/// the moment a lagging member is told to fetch the lot.
const NOTICE_BUFFER: usize = 64;

/// Keeper's fan-out to connected members.
#[derive(Clone)]
pub(crate) struct FederationEventBus {
    sender: broadcast::Sender<FederationChangeNotice>,
}

impl FederationEventBus {
    pub(crate) fn new() -> Self {
        let (sender, _) = broadcast::channel(NOTICE_BUFFER);
        Self { sender }
    }

    /// Ring the doorbell. Returns without error when nobody is listening,
    /// because a change with no connected member is ordinary rather than a
    /// failure — that member will poll when it reconnects.
    pub(crate) fn announce(&self, notice: FederationChangeNotice) {
        let _ = self.sender.send(notice);
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<FederationChangeNotice> {
        self.sender.subscribe()
    }
}

impl Default for FederationEventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// A member opening its one event connection.
///
/// Authenticated by the same node credential every other federation route
/// uses, and rechecked here rather than trusted from the upgrade: a socket that
/// outlives a revoked credential would be exactly the standing authority doc 98
/// refuses.
pub(crate) async fn federation_events(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?.to_owned();
    let service = apiary_service(&state)?;
    // Authenticate BEFORE upgrading. An unauthenticated socket that is later
    // rejected has already cost a connection and told the caller it succeeded.
    let apiary_id = service
        .authenticated_member_apiary(&credential, unix_timestamp())
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_federation_credential",
                "a current federation node credential is required",
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
    let receiver = state.federation_events.subscribe();
    Ok(websocket.on_upgrade(move |socket| async move {
        serve_notices(socket, receiver, apiary_id).await;
        drop(permit);
    }))
}

/// Forwards notices for one Apiary until the socket closes.
async fn serve_notices(
    mut socket: WebSocket,
    mut receiver: broadcast::Receiver<FederationChangeNotice>,
    apiary_id: swarm_domain::ApiaryId,
) {
    loop {
        let notice = match receiver.recv().await {
            Ok(notice) => notice,
            // ⚠️ LAGGING IS RECOVERABLE PRECISELY BECAUSE THIS IS A DOORBELL.
            // The member provably missed notices, and the honest recovery is to
            // ring once for everything rather than guess which were lost.
            // Reconciling more than necessary costs a poll; reconciling less
            // loses a change.
            Err(broadcast::error::RecvError::Lagged(_)) => FederationChangeNotice::new(
                FederationChangeKind::Unspecified,
                apiary_id,
                unix_timestamp(),
            ),
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if notice.apiary_id != apiary_id {
            continue;
        }
        let Ok(encoded) = serde_json::to_string(&notice) else {
            continue;
        };
        if socket.send(Message::Text(encoded.into())).await.is_err() {
            return;
        }
    }
}

/// How long a member waits before redialling, and the ceiling it backs off to.
///
/// ⚠️ BACKOFF EXISTS TO PROTECT KEEPER, NOT THE MEMBER. A member that redials
/// instantly against an unreachable or overloaded Keeper is a member that turns
/// one outage into a stampede. Missing the doorbell costs only latency, so
/// waiting longer is always the safe direction here.
const RECONNECT_FLOOR: Duration = Duration::from_secs(2);
const RECONNECT_CEILING: Duration = Duration::from_secs(120);

/// How long one connection is held before being redialled.
///
/// ⚠️ BOUNDED ON PURPOSE, FOR TWO REASONS. The background service owner requires
/// every service to finish its pass promptly so shutdown can join it; an
/// infinite socket loop is precisely what its tripwire exists to stop. And it
/// answers the proxy reality this task flagged: intermediaries routinely kill
/// idle sockets, so redialling on our own schedule turns an unpredictable
/// remote failure into a predictable local one.
const LISTEN_WINDOW: Duration = Duration::from_secs(60);

/// One bounded pass of the member's outbound event connection.
///
/// Connect, reconcile, listen for a while, return. Every disconnect is
/// ordinary: Keeper restarts, proxies time sockets out, laptops sleep. The pass
/// RECONCILES ON EVERY CONNECT, which is what makes a missed notice harmless —
/// anything that changed while the socket was down is picked up the moment it
/// comes back, with no need to know what was missed.
pub async fn poll_member_events(state: &AppState, backoff: &mut Duration) {
    // A failed dial must not become a tight redial loop against a Keeper that
    // is down; the caller's period plus this backoff is the total wait.
    if *backoff > RECONNECT_FLOOR {
        tokio::time::sleep(*backoff).await;
    }
    match tokio::time::timeout(LISTEN_WINDOW, connect_and_listen(state)).await {
        // Two ways to end well, sharing one body: the listen window elapsed with
        // the socket healthy, or Keeper closed it cleanly. Neither is a failure,
        // so neither backs off.
        Err(_) | Ok(Ok(())) => *backoff = RECONNECT_FLOOR,
        Ok(Err(error)) => {
            tracing::debug!(%error, "federation event socket dropped; will redial");
            *backoff = (*backoff * 2).min(RECONNECT_CEILING);
        }
    }
}

async fn connect_and_listen(state: &AppState) -> Result<(), String> {
    let service = apiary_service(state).map_err(|_| "no apiary service".to_owned())?;
    let connection = service
        .federation_member_connection()
        .map_err(|_| "not a federation member".to_owned())?;
    let url = events_url(&connection.keeper_endpoint)?;
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("invalid event endpoint: {error}"))?;
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        axum::http::HeaderValue::from_str(&format!("Bearer {}", connection.node_credential))
            .map_err(|_| "credential is not a valid header".to_owned())?,
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| error.to_string())?;

    // ⚠️ RECONCILE ON CONNECT, BEFORE LISTENING. Whatever changed while the
    // socket was down produced a notice nobody received. Fetching once here is
    // what lets every other part of this design treat a lost notice as
    // harmless.
    state.reconcile_federation().await;

    while let Some(message) = socket.next().await {
        match message.map_err(|error| error.to_string())? {
            ClientMessage::Text(_) => {
                // The notice names which feed moved, and this deliberately does
                // not read it: the reconciliation owner already fetches every
                // feed and applies each atomically. Acting on the KIND would be
                // the first step toward trusting the socket's contents.
                state.reconcile_federation().await;
            }
            ClientMessage::Close(_) => return Ok(()),
            // Ping/pong are handled by the library; anything else is noise a
            // doorbell has no use for.
            _ => {}
        }
    }
    Ok(())
}

/// The event endpoint for a Keeper base URL, upgraded from http(s) to ws(s).
fn events_url(keeper_endpoint: &str) -> Result<String, String> {
    let trimmed = keeper_endpoint.trim_end_matches('/');
    let base = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        // Plain HTTP is accepted only where the federation client already
        // accepts it: loopback development peers.
        format!("ws://{rest}")
    } else {
        return Err("keeper endpoint is not http(s)".to_owned());
    };
    Ok(format!("{base}/api/v1/federation/events"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_endpoint_upgrades_the_scheme_and_keeps_the_path() {
        assert_eq!(
            events_url("https://keeper.example.com").unwrap(),
            "wss://keeper.example.com/api/v1/federation/events"
        );
        assert_eq!(
            events_url("https://keeper.example.com/").unwrap(),
            "wss://keeper.example.com/api/v1/federation/events",
            "a trailing slash must not produce a doubled path"
        );
        assert_eq!(
            events_url("http://127.0.0.1:8787").unwrap(),
            "ws://127.0.0.1:8787/api/v1/federation/events"
        );
        assert!(events_url("keeper.example.com").is_err());
        assert!(events_url("ftp://keeper.example.com").is_err());
    }

    /// ⚠️ BACKOFF MUST ACTUALLY BOUND. A ceiling that a doubling sequence can
    /// step over turns a protective delay into an unbounded one.
    #[test]
    fn reconnect_backoff_doubles_and_stops_at_the_ceiling() {
        let mut wait = RECONNECT_FLOOR;
        let mut seen = Vec::new();
        for _ in 0..12 {
            wait = (wait * 2).min(RECONNECT_CEILING);
            seen.push(wait);
        }
        assert!(seen.iter().all(|value| *value <= RECONNECT_CEILING));
        assert_eq!(*seen.last().unwrap(), RECONNECT_CEILING);
        assert!(seen[0] > RECONNECT_FLOOR, "it must actually back off");
    }

    /// A notice carries no state, so a member cannot act on its contents even by
    /// accident. If this ever fails, the doorbell has become a pipe.
    #[test]
    fn a_notice_carries_no_state_a_member_could_act_on() {
        let notice = FederationChangeNotice::new(
            FederationChangeKind::Tasks,
            swarm_domain::ApiaryId::new(),
            1_790_000_000,
        );
        let encoded = serde_json::to_value(&notice).unwrap();
        let object = encoded.as_object().unwrap();
        // Compared as a SET: serde_json orders keys itself, and what matters
        // here is which fields exist, not the order they serialise in.
        let mut fields = object.keys().map(String::as_str).collect::<Vec<_>>();
        fields.sort_unstable();
        assert_eq!(
            fields,
            vec!["announced_at", "apiary_id", "kind"],
            "a notice must name what moved and nothing more"
        );
    }
}
