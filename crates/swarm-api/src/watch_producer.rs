//! The watched Hive's side of the live window: it sends its own frames out.
//!
//! ⚠️ THE MEMBER PUSHES; KEEPER NEVER REACHES IN. Every federation connection in
//! Swarm is outbound from the member, and this is no exception — a watch is
//! Keeper being ALLOWED to see, not Keeper being able to take. If this ever
//! becomes an inbound connection, the one-operator Hive boundary has gone with
//! it.
//!
//! ⚠️ NOTHING IS WRITTEN DOWN HERE EITHER. This reads the terminal the Hive
//! already keeps for its own operator and forwards bytes. It adds no store, no
//! file, and no second copy of anything.

use std::time::Duration;

use futures_util::SinkExt;
use swarm_domain::ApiaryWatchId;
use swarm_terminal::{HostRequest, HostResponse, Resume};
use tokio_tungstenite::tungstenite::{Message as ClientMessage, client::IntoClientRequest};

use crate::{AppState, apiary_service, task_store, unix_timestamp};

/// How long one relay pass holds its connection before returning to the caller.
///
/// The same shape as the event socket: a bounded window rather than an
/// unbounded task, so the service that owns it can be stopped.
const RELAY_WINDOW: Duration = Duration::from_secs(60);

/// How often the watched terminal is read while a window is open.
///
/// ⚠️ A CADENCE, NOT A CORRECTNESS MECHANISM. Frames are sequenced and the
/// reader advances by sequence, so a slow tick shows a late screen rather than
/// a wrong one. 200ms is fast enough to read as live and slow enough that an
/// idle Hive is not asking its terminal host twenty times a second.
const FRAME_POLL: Duration = Duration::from_millis(200);

const OUTPUT_FRAME_TYPE: u8 = 1;
const SNAPSHOT_FRAME_TYPE: u8 = 2;

/// Sends this Hive's Queen terminal to whoever it has acknowledged watching it.
///
/// ⚠️ QUEEN'S TERMINAL, WHICH IS NARROWER THAN THE OPERATOR ASKED FOR. ADR 0036
/// relays exactly this for takeover and it is the bounded surface that already
/// has a precedent; "literally everything" means every Hive rather than every
/// pane, and widening to a chosen session needs the watcher to be able to ASK
/// for one, which this one-way push deliberately cannot carry. Say so rather
/// than let a reader assume the window is wider than it is.
pub(crate) async fn relay_watched_frames(state: &AppState) {
    let Ok(store) = task_store(state) else {
        return;
    };
    let now = unix_timestamp();
    let Ok(watches) = store.local_open_watches(now) else {
        return;
    };
    // Only an acknowledged watch relays. A `requested` one has not yet been
    // shown to this Hive's operator, and showing it is the precondition.
    let Some(watch) = watches.into_iter().find(|watch| watch.is_live(now)) else {
        return;
    };
    if let Err(error) = relay_one(state, watch.id).await {
        tracing::debug!(%error, "watch relay dropped; will redial");
    }
}

async fn relay_one(state: &AppState, watch: ApiaryWatchId) -> Result<(), String> {
    let service = apiary_service(state).map_err(|_| "no apiary service".to_owned())?;
    let connection = service
        .federation_member_connection()
        .map_err(|_| "not a federation member".to_owned())?;
    let url = frames_url(&connection.keeper_endpoint, watch)?;
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("invalid relay endpoint: {error}"))?;
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        axum::http::HeaderValue::from_str(&format!("Bearer {}", connection.node_credential))
            .map_err(|_| "credential is not a valid header".to_owned())?,
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| error.to_string())?;

    let Some(host) = state.terminal_host.as_ref() else {
        return Ok(());
    };
    let Ok(store) = task_store(state) else {
        return Ok(());
    };
    let Ok(Some(session)) = store.active_queen_session_id() else {
        // Nothing is running to look at. Not an error — an idle Hive is a
        // perfectly ordinary thing to be watching.
        return Ok(());
    };

    let deadline = tokio::time::Instant::now() + RELAY_WINDOW;
    let mut after: Option<u64> = None;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        // Re-read the lease every pass. The watched operator can end a watch
        // from their own machine at any moment, and this side must stop
        // producing without waiting for Keeper to hang up on it.
        let now = unix_timestamp();
        let still_watched = store.local_open_watches(now).is_ok_and(|open| {
            open.iter()
                .any(|held| held.id == watch && held.is_live(now))
        });
        if !still_watched {
            return Ok(());
        }
        let response = host
            .request(&HostRequest::Read {
                session_id: session,
                after_sequence: after,
            })
            .await
            .map_err(|error| error.to_string())?;
        if let HostResponse::Output { resume, .. } | HostResponse::ControlledOutput { resume, .. } =
            response
        {
            for frame in encode(&resume, &mut after) {
                socket
                    .send(ClientMessage::Binary(frame.into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        tokio::time::sleep(FRAME_POLL).await;
    }
}

/// Encodes what the host returned in the SAME wire format the local terminal
/// socket uses.
///
/// ⚠️ DELIBERATELY IDENTICAL, so a watcher renders a remote Hive with the same
/// code that renders their own. A second frame format would be a second thing
/// to keep correct, and the two would drift the first time either changed.
fn encode(resume: &Resume, after: &mut Option<u64>) -> Vec<Vec<u8>> {
    match resume {
        Resume::Snapshot { snapshot } => {
            let mut payload = Vec::with_capacity(snapshot.bytes.len() + 14);
            payload.push(SNAPSHOT_FRAME_TYPE);
            payload.extend_from_slice(&snapshot.sequence.to_be_bytes());
            payload.extend_from_slice(&snapshot.rows.to_be_bytes());
            payload.extend_from_slice(&snapshot.columns.to_be_bytes());
            payload.push(u8::from(snapshot.truncated));
            payload.extend_from_slice(&snapshot.bytes);
            *after = Some(snapshot.sequence);
            vec![payload]
        }
        Resume::Deltas { frames } => frames
            .iter()
            .map(|frame| {
                let mut payload = Vec::with_capacity(frame.bytes.len() + 9);
                payload.push(OUTPUT_FRAME_TYPE);
                payload.extend_from_slice(&frame.sequence.to_be_bytes());
                payload.extend_from_slice(&frame.bytes);
                *after = Some(frame.sequence);
                payload
            })
            .collect(),
    }
}

fn frames_url(keeper_endpoint: &str, watch: ApiaryWatchId) -> Result<String, String> {
    let trimmed = keeper_endpoint.trim_end_matches('/');
    let base = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err("keeper endpoint is not http(s)".to_owned());
    };
    Ok(format!("{base}/api/v1/federation/watches/{watch}/frames"))
}

#[cfg(test)]
mod tests {
    use swarm_terminal::{SequencedFrame, TerminalSnapshot};

    use super::*;

    /// ⚠️ BYTE-FOR-BYTE WHAT THE LOCAL TERMINAL SOCKET SENDS. If this drifts, a
    /// watcher renders a remote Hive with a decoder that no longer matches, and
    /// the failure is a garbled screen rather than an error.
    #[test]
    fn frames_are_encoded_exactly_as_the_local_terminal_socket_encodes_them() {
        let mut after = None;
        let deltas = Resume::Deltas {
            frames: vec![
                SequencedFrame {
                    sequence: 7,
                    bytes: b"ls".to_vec(),
                },
                SequencedFrame {
                    sequence: 8,
                    bytes: b"\n".to_vec(),
                },
            ],
        };
        let encoded = encode(&deltas, &mut after);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0][0], OUTPUT_FRAME_TYPE);
        assert_eq!(u64::from_be_bytes(encoded[0][1..9].try_into().unwrap()), 7);
        assert_eq!(&encoded[0][9..], b"ls");
        assert_eq!(
            after,
            Some(8),
            "the reader advances, or the next pass resends what it just sent"
        );

        let snapshot = Resume::Snapshot {
            snapshot: TerminalSnapshot {
                sequence: 12,
                rows: 24,
                columns: 80,
                truncated: true,
                bytes: b"screen".to_vec(),
            },
        };
        let encoded = encode(&snapshot, &mut after);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0][0], SNAPSHOT_FRAME_TYPE);
        assert_eq!(u64::from_be_bytes(encoded[0][1..9].try_into().unwrap()), 12);
        assert_eq!(
            u16::from_be_bytes(encoded[0][9..11].try_into().unwrap()),
            24
        );
        assert_eq!(
            u16::from_be_bytes(encoded[0][11..13].try_into().unwrap()),
            80
        );
        assert_eq!(
            encoded[0][13], 1,
            "truncation is carried, not silently dropped"
        );
        assert_eq!(&encoded[0][14..], b"screen");
        assert_eq!(after, Some(12));
    }

    /// Plain HTTP is upgraded only where the federation client already accepts
    /// it, and anything that is not http(s) is refused rather than guessed at.
    #[test]
    fn the_relay_endpoint_follows_the_keeper_scheme() {
        let watch = ApiaryWatchId::new();
        assert_eq!(
            frames_url("https://keeper.example/", watch).unwrap(),
            format!("wss://keeper.example/api/v1/federation/watches/{watch}/frames")
        );
        assert_eq!(
            frames_url("http://127.0.0.1:8010", watch).unwrap(),
            format!("ws://127.0.0.1:8010/api/v1/federation/watches/{watch}/frames")
        );
        assert!(frames_url("keeper.example", watch).is_err());
    }
}
