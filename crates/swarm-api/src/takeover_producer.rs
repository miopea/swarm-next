//! The held Hive's side of a takeover: its screen out, the controller's
//! keystrokes in.
//!
//! ⚠️ THIS HALF WAS MISSING, AND TAKEOVER WAS A BLANK WINDOW WITHOUT IT. Keeper
//! relayed both directions and the controller's window attached, but nothing on
//! the held Hive ever dialled the relay: no frame ever arrived and every
//! keystroke went into a channel nobody read. The operator's live check on
//! 2026-09-24 was "takeover starts and stops, the screen is blank".
//!
//! ⚠️ THE MEMBER DIALS OUT, as it does for watching. Keeper never reaches in.
//!
//! ⚠️ THE TERMINAL HOST IS THE BOUNDARY, NOT THIS LOOP. Keystrokes are written
//! only through `TakeoverWrite`, which the host accepts only under the exact
//! installed lease, and the local operator's reclaim removes that authority on
//! the host directly. A bug here can fail to deliver input; it cannot deliver
//! input the lease does not authorise.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use swarm_domain::{FederationStewardTakeoverLease, FederationStewardTakeoverLeaseId};
use swarm_terminal::{
    HostRequest, HostResponse, MAX_CONTROL_INPUT_BYTES, TerminalControlCommand,
    TerminalTakeoverLease,
};
use tokio_tungstenite::tungstenite::{Message as ClientMessage, client::IntoClientRequest};

use crate::{AppState, apiary_service, task_store, unix_timestamp, watch_producer};

/// How long one relay pass holds its connection before returning to the caller.
///
/// Bounded like the watch relay, so the service that owns it can be stopped.
/// Each redial starts from a fresh snapshot.
const RELAY_WINDOW: Duration = Duration::from_secs(60);

/// How often the held terminal is read while a takeover is live.
///
/// A cadence, not a correctness mechanism: frames are sequenced and the reader
/// advances by sequence, so a slow tick shows a late screen, never a wrong one.
const FRAME_POLL: Duration = Duration::from_millis(100);

/// A keystroke from whoever holds the lease, exactly as their window sends it.
pub(crate) const INPUT_FRAME_TYPE: u8 = 9;

/// Keeper asking for the whole screen again, because a window just attached.
///
/// ⚠️ WITHOUT THIS A WINDOW OPENED AFTER THE HELD HIVE CONNECTED STARTS EMPTY.
/// The relay forwards live frames and keeps none, so the snapshot sent on
/// connect is gone by the time a later window subscribes, and deltas alone
/// draw fragments of a screen rather than the screen.
pub(crate) const RESNAPSHOT_FRAME_TYPE: u8 = 10;

/// Relays this Hive's Queen terminal while a takeover of it is live.
///
/// Returns promptly when nothing holds this Hive, which is the ordinary case.
pub(crate) async fn relay_held_terminal(state: &AppState) {
    let Ok(store) = task_store(state) else {
        return;
    };
    let Ok(Some(lease)) = store.held_takeover(unix_timestamp()) else {
        return;
    };
    if let Err(error) = relay_one(state, lease).await {
        tracing::debug!(%error, "takeover relay dropped; will redial");
    }
}

async fn relay_one(state: &AppState, lease: FederationStewardTakeoverLease) -> Result<(), String> {
    let Some(host) = state.terminal_host.as_ref() else {
        return Ok(());
    };
    let store = task_store(state).map_err(|_| "task store unavailable".to_owned())?;
    let Ok(Some(session)) = store.active_queen_session_id() else {
        // Nothing running to control; restart reconciliation ends the lease.
        return Ok(());
    };
    let service = apiary_service(state).map_err(|_| "no apiary service".to_owned())?;
    let connection = service
        .federation_member_connection()
        .map_err(|_| "not a federation member".to_owned())?;

    // Authority first, then the connection: a window must never show a
    // takeover as live while this Hive's own browser can still type into it.
    let mut installed = install_authority(host, session, &lease).await?;

    let url = relay_url(&connection.keeper_endpoint, lease.id, lease.revision)?;
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

    let deadline = tokio::time::Instant::now() + RELAY_WINDOW;
    let mut after: Option<u64> = None;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Ok(());
        }
        // Re-read every pass: a reclaim from this machine, a release, or expiry
        // must stop the relay without waiting for Keeper to hang up.
        let Ok(Some(current)) = store.held_takeover(unix_timestamp()) else {
            return Ok(());
        };
        if current.id != lease.id {
            return Ok(());
        }
        // A keystroke renews the lease at Keeper, and the host's copy of it
        // carries the old expiry until it is told the new one.
        if current.revision != installed.revision || current.expires_at != installed.expires_at {
            installed = install_authority(host, session, &current).await?;
        }
        tokio::select! {
            message = socket.next() => match message {
                Some(Ok(ClientMessage::Binary(frame))) => match frame.first() {
                    Some(&INPUT_FRAME_TYPE) => {
                        write_input(host, session, &installed, &frame[1..]).await;
                    }
                    Some(&RESNAPSHOT_FRAME_TYPE) => after = None,
                    _ => {}
                },
                Some(Ok(ClientMessage::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.to_string()),
            },
            () = tokio::time::sleep(FRAME_POLL) => {
                let response = host
                    .request(&HostRequest::Read {
                        session_id: session,
                        after_sequence: after,
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                if let HostResponse::Output { resume, .. }
                | HostResponse::ControlledOutput { resume, .. } = response
                {
                    for frame in watch_producer::encode(&resume, &mut after) {
                        socket
                            .send(ClientMessage::Binary(frame.into()))
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
        }
    }
}

/// Hands the Queen terminal to the lease, taking it from this Hive's own view.
///
/// ⚠️ ACTIVATION REPLACES THE LOCAL VIEW, AS ADR 0036 REQUIRES. The host refuses
/// to install remote authority while a local browser owns the terminal, and a
/// local view renews itself for as long as the tab is open, so waiting for it
/// to lapse would mean a takeover never starts while the operator is looking.
/// Once installed, the host refuses that view's reclaim of control; the
/// operator takes the terminal back through "Take back control", which is
/// audited and reasoned.
async fn install_authority(
    host: &swarm_terminal::HostClient,
    session: swarm_domain::WorkerSessionId,
    lease: &FederationStewardTakeoverLease,
) -> Result<TerminalTakeoverLease, String> {
    let authority = TerminalTakeoverLease {
        lease_id: lease.id,
        revision: lease.revision,
        expires_at: lease.expires_at,
    };
    if let Ok(HostResponse::Control { control, .. }) = host
        .request(&HostRequest::Control {
            session_id: session,
            command: TerminalControlCommand::Status,
        })
        .await
        && let Some(owner) = control.owner
    {
        // Losing this race to a renewal is fine: the install below refuses and
        // the next pass tries again.
        let _ = host
            .request(&HostRequest::Control {
                session_id: session,
                command: TerminalControlCommand::Release {
                    identity: owner,
                    generation: control.generation,
                },
            })
            .await;
    }
    match host
        .request(&HostRequest::InstallTakeover {
            session_id: session,
            lease: authority,
        })
        .await
        .map_err(|error| error.to_string())?
    {
        HostResponse::Acknowledged => Ok(authority),
        HostResponse::Error { code, .. } => {
            Err(format!("the terminal host refused the takeover: {code}"))
        }
        _ => Err("unexpected terminal host response to a takeover install".to_owned()),
    }
}

/// Writes one keystroke under the installed lease.
///
/// ⚠️ NEVER RETRIED. A refused or failed write may have accepted a prefix, and
/// replaying it would type the same keys twice into someone else's machine.
async fn write_input(
    host: &swarm_terminal::HostClient,
    session: swarm_domain::WorkerSessionId,
    authority: &TerminalTakeoverLease,
    bytes: &[u8],
) {
    if bytes.is_empty() || bytes.len() > MAX_CONTROL_INPUT_BYTES {
        return;
    }
    let refused = match host
        .request(&HostRequest::TakeoverWrite {
            session_id: session,
            lease_id: authority.lease_id,
            revision: authority.revision,
            bytes: bytes.to_vec(),
        })
        .await
    {
        Ok(HostResponse::Acknowledged) => return,
        Ok(HostResponse::Error { code, .. }) => code,
        Ok(_) => "unexpected response".to_owned(),
        Err(error) => error.to_string(),
    };
    tracing::debug!(lease = %authority.lease_id, %refused, "a takeover keystroke was not written");
}

fn relay_url(
    keeper_endpoint: &str,
    lease: FederationStewardTakeoverLeaseId,
    revision: u64,
) -> Result<String, String> {
    let trimmed = keeper_endpoint.trim_end_matches('/');
    let base = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err("keeper endpoint is not http(s)".to_owned());
    };
    Ok(format!(
        "{base}/api/v1/federation/takeovers/{lease}/relay?revision={revision}"
    ))
}
