//! The live window itself: frames from a watched Hive to whoever is watching.
//!
//! ⚠️ NOTHING HERE TOUCHES A DATABASE, AND THAT IS THE DESIGN RATHER THAN AN
//! OPTIMISATION. ADR 0107 records the session and forbids recording the frames;
//! this module is where that promise is either kept or broken. It holds frames
//! in a bounded in-memory channel that dies with the process, and it treats
//! every frame as OPAQUE BYTES — it never parses one, so there is nothing here
//! that could grow into a transcript.
//!
//! If a future change gives this module a `TaskStore`, that change is
//! reversing the operator's bargain and should say so out loud.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::StreamExt;
use swarm_domain::ApiaryWatchId;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::{ApiError, AppState};

/// How many frames may be in flight to one viewer before it is considered lost.
///
/// ⚠️ SMALL ON PURPOSE. This is a LIVE window, so a viewer that has fallen 64
/// frames behind does not want them — it wants what is on screen now. Buffering
/// generously would turn a bounded relay into an accidental recent-history
/// buffer, which is the thing this module exists not to be. A viewer that lags
/// is dropped and reconnects to a fresh snapshot.
const RELAY_FRAME_BUFFER: usize = 64;

/// How often a socket re-checks that the watch authorizing it is still live.
///
/// The lease is the thing that stops a forgotten window becoming standing
/// surveillance, so a socket opened under one must notice it lapsing. Checked
/// rather than trusted from connect time, because a watch can also be ended
/// from the watched machine at any moment.
pub(crate) const RELAY_LIVENESS_CHECK_SECONDS: u64 = 15;

/// Bounded, memory-only fan-out of opaque frames, keyed by whatever names one
/// live connection.
///
/// ⚠️ GENERIC BECAUSE TAKEOVER NEEDS THE SAME THING AND MUST NOT GROW A SECOND
/// ONE. Watching is keyed by watch, takeover by lease, and the safety property
/// — frames pass through and are never kept — is identical. Two copies would be
/// two places for that property to stop being true, and only one of them would
/// get the next fix.
#[derive(Debug)]
pub(crate) struct FrameRelay<K> {
    channels: Mutex<HashMap<K, broadcast::Sender<Vec<u8>>>>,
}

impl<K> Default for FrameRelay<K> {
    fn default() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }
}

/// The relay carrying one watched Hive's screen to whoever is watching.
pub(crate) type WatchRelay = FrameRelay<ApiaryWatchId>;

impl<K: Copy + Eq + std::hash::Hash> FrameRelay<K> {
    /// Publishes one frame to whoever is currently watching.
    ///
    /// Returns without error when nobody is attached: a Hive whose watcher has
    /// closed their window is ordinary rather than a failure, and the member
    /// stops producing on its own when the watch ends.
    pub(crate) fn publish(&self, watch: K, frame: Vec<u8>) {
        let Ok(channels) = self.channels.lock() else {
            return;
        };
        if let Some(sender) = channels.get(&watch) {
            let _ = sender.send(frame);
        }
    }

    /// Attaches a viewer, creating the channel if this is the first one.
    pub(crate) fn subscribe(&self, watch: K) -> broadcast::Receiver<Vec<u8>> {
        let mut channels = match self.channels.lock() {
            Ok(channels) => channels,
            // A poisoned lock must not take the relay down: the honest failure
            // for a live window is an empty one the viewer can retry, not a
            // panic that also loses every other watch in flight.
            Err(poisoned) => poisoned.into_inner(),
        };
        channels
            .entry(watch)
            .or_insert_with(|| broadcast::channel(RELAY_FRAME_BUFFER).0)
            .subscribe()
    }

    /// Forgets a watch entirely.
    ///
    /// Called when a watch ends, so a relay that nobody closed cannot keep a
    /// channel alive indefinitely — the map is the only unbounded thing here,
    /// and this is what bounds it.
    pub(crate) fn retire(&self, watch: K) {
        let mut channels = match self.channels.lock() {
            Ok(channels) => channels,
            Err(poisoned) => poisoned.into_inner(),
        };
        channels.remove(&watch);
    }

    /// How many watches currently hold a channel. For tests and diagnostics.
    #[cfg(test)]
    pub(crate) fn live_channels(&self) -> usize {
        self.channels.lock().map_or(0, |channels| channels.len())
    }
}

/// How long a viewer has to use a grant before it lapses.
///
/// Matches the terminal attach grant rather than inventing a second number. It
/// is the gap between asking for a window and opening it, not the length of the
/// watch — the LEASE bounds that.
const WATCH_GRANT_TTL: Duration = Duration::from_secs(30);

/// A ceiling, so a bug that issues grants in a loop cannot grow this without
/// bound. Generous next to the number of windows anyone opens by hand.
const MAX_WATCH_GRANTS: usize = 64;

const WATCH_GRANT_PROTOCOL_PREFIX: &str = "swarm-watch.";

/// The subprotocol a browser offers to open a takeover control channel.
pub(crate) const TAKEOVER_GRANT_PROTOCOL_PREFIX: &str = "swarm-takeover.";

/// Short-lived single-use tickets that let a BROWSER open a viewer socket.
///
/// ⚠️ THIS EXISTS BECAUSE A BROWSER CANNOT SEND AN `Authorization` HEADER ON A
/// WEBSOCKET. The first version of the viewer route read one, which meant it
/// could never have been reached from the UI it was built for — it worked only
/// from a test that could set headers. Putting the operator token in a
/// subprotocol instead would leak a long-lived credential into a string that
/// proxies and logs routinely record, which is exactly what a single-use,
/// 30-second ticket avoids.
#[derive(Debug)]
pub(crate) struct GrantStore<K> {
    grants: Mutex<HashMap<String, (K, std::time::Instant)>>,
}

impl<K> Default for GrantStore<K> {
    fn default() -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
        }
    }
}

/// Tickets for a browser opening a watch viewer.
pub(crate) type WatchGrantStore = GrantStore<ApiaryWatchId>;

impl<K: Copy + Eq> GrantStore<K> {
    fn issue_at(&self, watch: K, now: std::time::Instant) -> Option<String> {
        let mut grants = match self.grants.lock() {
            Ok(grants) => grants,
            Err(poisoned) => poisoned.into_inner(),
        };
        grants.retain(|_, (_, expires)| *expires > now);
        if grants.len() >= MAX_WATCH_GRANTS {
            return None;
        }
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).ok()?;
        let mut token = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write;
            write!(&mut token, "{byte:02x}").ok()?;
        }
        grants.insert(token.clone(), (watch, now + WATCH_GRANT_TTL));
        Some(token)
    }

    /// Spends a grant. A grant is good for ONE socket: replaying it must not
    /// open a second window, so it is removed whether or not it matched.
    fn consume_at(&self, token: &str, watch: K, now: std::time::Instant) -> bool {
        let mut grants = match self.grants.lock() {
            Ok(grants) => grants,
            Err(poisoned) => poisoned.into_inner(),
        };
        grants.retain(|_, (_, expires)| *expires > now);
        grants
            .remove(token)
            .is_some_and(|(granted, _)| granted == watch)
    }

    pub(crate) fn issue(&self, watch: K) -> Option<String> {
        self.issue_at(watch, std::time::Instant::now())
    }

    pub(crate) fn consume(&self, token: &str, watch: K) -> bool {
        self.consume_at(token, watch, std::time::Instant::now())
    }
}

fn offered_grant(headers: &HeaderMap) -> Option<&str> {
    grant_with_prefix(headers, WATCH_GRANT_PROTOCOL_PREFIX)
}

/// Reads a single-use grant offered as a WebSocket subprotocol.
pub(crate) fn grant_with_prefix<'a>(headers: &'a HeaderMap, prefix: &str) -> Option<&'a str> {
    headers
        .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix(prefix))
}

/// Whether this watch still authorizes a socket, read fresh.
///
/// ⚠️ RE-READ RATHER THAN TRUSTED FROM CONNECT TIME. A lease can lapse, and the
/// watched operator can end it from their own machine at any moment; a socket
/// that kept relaying on the strength of having once been authorized is the
/// standing window this design exists to prevent.
fn watch_is_live(state: &AppState, watch: ApiaryWatchId, now: i64) -> bool {
    crate::task_store(state).is_ok_and(|store| {
        store.apiary_watch_audit(200).is_ok_and(|watches| {
            watches
                .iter()
                .any(|held| held.id == watch && held.is_live(now))
        })
    })
}

/// The watched Hive pushing its own frames outward.
///
/// Member-authenticated, and refused unless the watch is ACTIVE and targets
/// this member — a Hive cannot stream on behalf of anyone else, and a watch its
/// operator has not been shown is not active yet.
pub(crate) async fn federation_watch_frames(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(watch_id): Path<ApiaryWatchId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let credential = crate::federation_node_credential(&headers)?.to_owned();
    let now = crate::unix_timestamp();
    let store = crate::task_store(&state)?;
    // Authenticate and authorize BEFORE upgrading, for the same reason the
    // event socket does: a socket rejected after the upgrade has already told
    // the caller it succeeded.
    let authorized = store
        .federation_watch_inbox(&credential, now)
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_federation_credential",
                "a current federation node credential is required",
            )
        })?
        .into_iter()
        .any(|watch| watch.id == watch_id && watch.is_live(now));
    if !authorized {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "watch_not_active",
            "no acknowledged watch on this Hive authorizes a relay",
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
    let relayed = Arc::clone(&state);
    Ok(websocket.on_upgrade(move |socket| async move {
        receive_frames(socket, relayed, watch_id).await;
        drop(permit);
    }))
}

/// Reads frames from the watched Hive and fans them out. Never parses one.
async fn receive_frames(mut socket: WebSocket, state: Arc<AppState>, watch: ApiaryWatchId) {
    let mut liveness = tokio::time::interval(Duration::from_secs(RELAY_LIVENESS_CHECK_SECONDS));
    liveness.tick().await;
    loop {
        tokio::select! {
            _ = liveness.tick() => {
                if !watch_is_live(&state, watch, crate::unix_timestamp()) {
                    state.watch_relay.retire(watch);
                    return;
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Binary(frame))) => {
                        state.watch_relay.publish(watch, frame.to_vec());
                    }
                    // A relay carries terminal bytes and nothing else. Text
                    // would be someone inventing a second protocol inside this
                    // one, so it is dropped rather than interpreted.
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => {
                        state.watch_relay.retire(watch);
                        return;
                    }
                }
            }
        }
    }
}

/// A Steward's own Hive attaching to the window, on its outbound connection.
///
/// ⚠️ THE SAME RELAY AND THE SAME LEASE AS THE KEEPER'S VIEWER. The only thing
/// that differs is how the caller proved who they are — a node credential
/// rather than a browser's grant — because a Steward's browser talks to their
/// own Hive and their Hive talks to Keeper. Depth does not vary with the route
/// you arrived on, which is the rule this whole capability is built around.
pub(crate) async fn federation_watch_stream(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(watch_id): Path<ApiaryWatchId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let credential = crate::federation_node_credential(&headers)?.to_owned();
    let now = crate::unix_timestamp();
    crate::task_store(&state)?
        .federation_watch_for_watcher(&credential, watch_id, now)
        .map_err(|_| {
            ApiError::new(
                StatusCode::FORBIDDEN,
                "watch_not_live",
                "no live watch of yours has that id",
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
    let receiver = state.watch_relay.subscribe(watch_id);
    let viewed = Arc::clone(&state);
    Ok(websocket.on_upgrade(move |socket| async move {
        serve_frames(socket, receiver, viewed, watch_id).await;
        drop(permit);
    }))
}

/// The watcher's window.
///
/// Authorized against the watch this operator holds, re-checked while it runs.
pub(crate) async fn apiary_watch_stream(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(watch_id): Path<ApiaryWatchId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let grant = offered_grant(&headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "watch_grant_required",
            "a short-lived watch grant is required",
        )
    })?;
    if !state.watch_grants.consume(grant, watch_id) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_watch_grant",
            "the watch grant is invalid, expired, or already used",
        ));
    }
    // ⚠️ A STEWARD'S HIVE HOLDS NO `apiary_watches` AND MUST NOT ANSWER FROM
    // THE ABSENCE. Checking here on a member refused every legitimate window,
    // because an empty table reads identically to a watch that ended. Keeper
    // owns this question and answers it when the proxy dials — the same reason
    // the grant route does not ask it either.
    let member = crate::local_apiary_role(&state) == Some(swarm_domain::LocalApiaryRole::Member);
    // Re-checked after the grant, not instead of it: a grant proves who asked,
    // and this proves the watch is still theirs and still live.
    if !member && !watch_is_live(&state, watch_id, crate::unix_timestamp()) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "watch_not_live",
            "that watch is no longer live",
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
    // ⚠️ A STEWARD'S HIVE HOLDS NO FRAMES OF ITS OWN. It is not the relay; the
    // relay is at Keeper. So a member PROXIES — it dials Keeper's viewer socket
    // outbound and forwards what arrives, which is the same hop every other
    // federation read takes and keeps the outbound-only model intact.
    let receiver = (!member).then(|| state.watch_relay.subscribe(watch_id));
    let viewed = Arc::clone(&state);
    // ⚠️ THE SELECTED SUBPROTOCOL MUST BE ECHOED. A client that offers one and
    // is answered with none fails the handshake — so omitting this made the
    // route unreachable from the browser it exists for, which is precisely the
    // shape of the header bug it replaced.
    let selected = format!("{WATCH_GRANT_PROTOCOL_PREFIX}{grant}");
    Ok(websocket
        .protocols([selected])
        .on_upgrade(move |socket| async move {
            match receiver {
                Some(receiver) => serve_frames(socket, receiver, viewed, watch_id).await,
                None => proxy_frames(socket, viewed, watch_id).await,
            }
            drop(permit);
        }))
}

/// Forwards Keeper's frames to a Steward's own browser.
///
/// Failures here close the window rather than being reported in detail: from
/// the watcher's side "the window closed" is the whole truth, and a Hive that
/// could not reach Keeper and one whose watch just ended look identical and
/// deserve the same words.
async fn proxy_frames(mut socket: WebSocket, state: Arc<AppState>, watch: ApiaryWatchId) {
    let Ok(service) = crate::apiary_service(&state) else {
        return;
    };
    let Ok(connection) = service.federation_member_connection() else {
        return;
    };
    let Ok(url) = keeper_stream_url(&connection.keeper_endpoint, watch) else {
        return;
    };
    let Ok(mut request) = url.into_client_request() else {
        return;
    };
    let Ok(bearer) =
        axum::http::HeaderValue::from_str(&format!("Bearer {}", connection.node_credential))
    else {
        return;
    };
    request
        .headers_mut()
        .insert(axum::http::header::AUTHORIZATION, bearer);
    let Ok((upstream, _)) = tokio_tungstenite::connect_async(request).await else {
        return;
    };
    let (_, mut inbound) = upstream.split();
    while let Some(Ok(message)) = inbound.next().await {
        if let tokio_tungstenite::tungstenite::Message::Binary(frame) = message
            && socket
                .send(Message::Binary(frame.to_vec().into()))
                .await
                .is_err()
        {
            return;
        }
    }
}

fn keeper_stream_url(keeper_endpoint: &str, watch: ApiaryWatchId) -> Result<String, ()> {
    let trimmed = keeper_endpoint.trim_end_matches('/');
    let base = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err(());
    };
    Ok(format!("{base}/api/v1/federation/watches/{watch}/stream"))
}

/// Forwards frames to one viewer until the watch ends or they fall behind.
async fn serve_frames(
    mut socket: WebSocket,
    mut receiver: broadcast::Receiver<Vec<u8>>,
    state: Arc<AppState>,
    watch: ApiaryWatchId,
) {
    let mut liveness = tokio::time::interval(Duration::from_secs(RELAY_LIVENESS_CHECK_SECONDS));
    liveness.tick().await;
    loop {
        tokio::select! {
            _ = liveness.tick() => {
                if !watch_is_live(&state, watch, crate::unix_timestamp()) {
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
                    // ⚠️ LAGGING IS NOT RECOVERABLE HERE, unlike the doorbell.
                    // A doorbell can ring once for everything it missed; a
                    // terminal cannot be resynchronised from a gap, and
                    // pretending otherwise would draw a screen that never
                    // existed. Closing sends the viewer back for a fresh
                    // snapshot, which is the only honest answer.
                    Err(
                        broadcast::error::RecvError::Lagged(_)
                        | broadcast::error::RecvError::Closed,
                    ) => return,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_reaches_every_attached_viewer_and_nothing_is_kept() {
        let relay = WatchRelay::default();
        let watch = ApiaryWatchId::new();
        let mut first = relay.subscribe(watch);
        let mut second = relay.subscribe(watch);

        relay.publish(watch, b"frame".to_vec());
        assert_eq!(first.try_recv().unwrap(), b"frame");
        assert_eq!(second.try_recv().unwrap(), b"frame");

        // ⚠️ A VIEWER WHO ARRIVES LATE GETS NOTHING, which is correct for a LIVE
        // window and is also the property that keeps this from being a
        // transcript. There is no backlog to hand them.
        let mut late = relay.subscribe(watch);
        assert!(late.try_recv().is_err(), "nothing was kept for them");
    }

    /// A publish with nobody attached is ordinary, not an error, and it must not
    /// create a channel that then lives forever.
    #[test]
    fn publishing_to_an_empty_watch_keeps_nothing_alive() {
        let relay = WatchRelay::default();
        relay.publish(ApiaryWatchId::new(), b"frame".to_vec());
        assert_eq!(relay.live_channels(), 0);
    }

    /// ⚠️ THE MAP IS THE ONLY UNBOUNDED THING IN THIS MODULE. Without retiring,
    /// every watch ever opened would keep a channel for the life of the
    /// process.
    #[test]
    fn retiring_a_watch_releases_its_channel() {
        let relay = WatchRelay::default();
        let watch = ApiaryWatchId::new();
        let _viewer = relay.subscribe(watch);
        assert_eq!(relay.live_channels(), 1);
        relay.retire(watch);
        assert_eq!(relay.live_channels(), 0);
    }

    /// A viewer that cannot keep up is DROPPED rather than buffered, so it
    /// reconnects to a fresh snapshot instead of slowly replaying a backlog
    /// nobody asked this module to keep.
    #[test]
    fn a_viewer_that_falls_behind_is_lagged_rather_than_buffered() {
        let relay = WatchRelay::default();
        let watch = ApiaryWatchId::new();
        let mut viewer = relay.subscribe(watch);
        for index in 0..(RELAY_FRAME_BUFFER + 10) {
            relay.publish(watch, vec![u8::try_from(index % 256).unwrap()]);
        }
        assert!(
            matches!(
                viewer.try_recv(),
                Err(broadcast::error::TryRecvError::Lagged(_))
            ),
            "the relay refuses to grow a history for a viewer who stopped reading"
        );
    }
}
