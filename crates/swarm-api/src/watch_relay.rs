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
use swarm_domain::ApiaryWatchId;
use tokio::sync::broadcast;

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

/// Per-watch fan-out. Memory only, for the lifetime of this process.
#[derive(Debug, Default)]
pub(crate) struct WatchRelay {
    channels: Mutex<HashMap<ApiaryWatchId, broadcast::Sender<Vec<u8>>>>,
}

impl WatchRelay {
    /// Publishes one frame to whoever is currently watching.
    ///
    /// Returns without error when nobody is attached: a Hive whose watcher has
    /// closed their window is ordinary rather than a failure, and the member
    /// stops producing on its own when the watch ends.
    pub(crate) fn publish(&self, watch: ApiaryWatchId, frame: Vec<u8>) {
        let Ok(channels) = self.channels.lock() else {
            return;
        };
        if let Some(sender) = channels.get(&watch) {
            let _ = sender.send(frame);
        }
    }

    /// Attaches a viewer, creating the channel if this is the first one.
    pub(crate) fn subscribe(&self, watch: ApiaryWatchId) -> broadcast::Receiver<Vec<u8>> {
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
    pub(crate) fn retire(&self, watch: ApiaryWatchId) {
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

/// The watcher's window.
///
/// Authorized against the watch this operator holds, re-checked while it runs.
pub(crate) async fn apiary_watch_stream(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(watch_id): Path<ApiaryWatchId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    crate::authorize(&state, &headers)?;
    let now = crate::unix_timestamp();
    let store = crate::task_store(&state)?;
    let watcher = store
        .local_hive_identity()
        .map_err(|error| crate::task_store_error(&error))?
        .operator
        .id;
    let authorized = store
        .apiary_watch_audit(200)
        .map_err(|error| crate::task_store_error(&error))?
        .into_iter()
        .any(|watch| {
            watch.id == watch_id && watch.watcher_operator_id == watcher && watch.is_live(now)
        });
    if !authorized {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "watch_not_live",
            "this operator holds no live watch with that id",
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
    let receiver = state.watch_relay.subscribe(watch_id);
    let viewed = Arc::clone(&state);
    Ok(websocket.on_upgrade(move |socket| async move {
        serve_frames(socket, receiver, viewed, watch_id).await;
        drop(permit);
    }))
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
