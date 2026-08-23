use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt, stream::SplitStream};
use serde::{Deserialize, Serialize};
use swarm_domain::{PresenceDeviceId, WorkerSessionId};
use swarm_persistence::TaskStore;
use swarm_terminal::{
    HostClient, HostRequest, HostResponse, MIN_TERMINAL_COLUMNS, MIN_TERMINAL_ROWS, Resume,
    TerminalSize, TerminalWriteProvenance,
};
use tokio::sync::{Notify, mpsc};

use axum::extract::ws::{Message, WebSocket};

pub const TERMINAL_WEBSOCKET_PROTOCOL: &str = "swarm-terminal.v3";
pub const TERMINAL_GRANT_PROTOCOL_PREFIX: &str = "swarm-grant.";
pub const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 64 * 1024;
const OUTBOUND_MESSAGE_CAPACITY: usize = 64;
const OUTPUT_FRAME_TYPE: u8 = 1;
const SNAPSHOT_FRAME_TYPE: u8 = 2;
pub const OPERATOR_ENGAGEMENT_LEASE_SECONDS: i64 = 300;
/// A claim made by arriving rather than by typing.
///
/// Shorter than the lease typing earns, and viewing does not renew it.
/// Engagement holds back the coordination a worker is owed, so a claim that
/// costs nothing to make must not silence a worker for as long as demonstrated
/// presence does. Typing converts it to a full lease through the existing path.
pub const VIEWING_ENGAGEMENT_LEASE_SECONDS: i64 = 90;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientTerminalMessage {
    Resume {
        after_sequence: Option<u64>,
        rows: u16,
        columns: u16,
        #[serde(default)]
        device_id: Option<PresenceDeviceId>,
        #[serde(default)]
        claim_geometry: bool,
    },
    Input {
        text: String,
    },
    Resize {
        rows: u16,
        columns: u16,
        #[serde(default)]
        claim_geometry: bool,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerTerminalMessage<'a> {
    State { running: bool, latest_sequence: u64 },
    Error { code: &'a str, message: String },
}

pub async fn serve_terminal_socket(
    mut socket: WebSocket,
    terminal_host: HostClient,
    session_id: WorkerSessionId,
    task_store: TaskStore,
    control_room_notify: Arc<Notify>,
) {
    let Some((after_sequence, initial_size, owner_device_id, claim_geometry)) =
        complete_initial_handshake(&mut socket).await
    else {
        return;
    };

    let (mut socket_sender, socket_receiver) = socket.split();
    let (outbound_sender, mut outbound_receiver) =
        mpsc::channel::<Message>(OUTBOUND_MESSAGE_CAPACITY);
    let writer = tokio::spawn(async move {
        while let Some(message) = outbound_receiver.recv().await {
            if socket_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    // Only the selected foreground terminal asks to replace stale geometry.
    // ResizeObserver messages remain owner-bound, so background desktop and
    // mobile viewers cannot oscillate the shared provider process afterward.
    if claim_geometry {
        let Some(owner_device_id) = owner_device_id else {
            let _ = send_control(
                &outbound_sender,
                &ServerTerminalMessage::Error {
                    code: "terminal_resize_authority_unavailable",
                    message: "terminal geometry claim requires an identified device".into(),
                },
            )
            .await;
            drop(outbound_sender);
            let _ = writer.await;
            return;
        };
        // Connecting is not a claim. This took the geometry from whoever held
        // it on every socket open, so opening a worker already visible on
        // another device stole its size — the desktop's next resize took it
        // back, and the operator watched a phone refresh itself repeatedly:
        // "I left this in desktop. Went to mobile and it just kept refreshing."
        //
        // Intent arrives in the resume and resize messages, which already
        // distinguish an explicit claim from an incidental one. Opening a
        // socket says only that someone is looking.
        match task_store.claim_unowned_worker_geometry(session_id, owner_device_id) {
            Ok(true) => {
                if !forward_host_request(
                    &terminal_host,
                    HostRequest::Resize {
                        session_id,
                        size: initial_size,
                    },
                    &outbound_sender,
                )
                .await
                {
                    drop(outbound_sender);
                    let _ = writer.await;
                    return;
                }
            }
            Ok(false) | Err(_) => {
                let _ = send_control(
                    &outbound_sender,
                    &ServerTerminalMessage::Error {
                        code: "terminal_resize_authority_unavailable",
                        message: "terminal geometry claim could not be recorded".into(),
                    },
                )
                .await;
            }
        }
    }

    let output_host = terminal_host.clone();
    let output_sender = outbound_sender.clone();
    let mut output = tokio::spawn(async move {
        stream_output(output_host, session_id, after_sequence, output_sender).await;
    });
    let mut input = tokio::spawn(handle_input(
        socket_receiver,
        TerminalInputContext {
            terminal_host,
            session_id,
            task_store,
            control_room_notify,
            outbound: outbound_sender.clone(),
            owner_device_id,
        },
        initial_size,
    ));

    let (input_completed, output_completed) = tokio::select! {
        _ = &mut input => (true, false),
        _ = &mut output => (false, true),
    };
    if !input_completed {
        input.abort();
        let _ = input.await;
    }
    if !output_completed {
        output.abort();
        let _ = output.await;
    }
    drop(outbound_sender);
    let _ = writer.await;
}

async fn complete_initial_handshake(
    socket: &mut WebSocket,
) -> Option<(Option<u64>, TerminalSize, Option<PresenceDeviceId>, bool)> {
    let initial = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;
    let (after_sequence, initial_size, owner_device_id, claim_geometry) = match initial {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<ClientTerminalMessage>(&text) {
                Ok(ClientTerminalMessage::Resume {
                    after_sequence,
                    rows,
                    columns,
                    device_id,
                    claim_geometry,
                }) if rows >= MIN_TERMINAL_ROWS && columns >= MIN_TERMINAL_COLUMNS => (
                    after_sequence,
                    TerminalSize::new(rows, columns),
                    device_id,
                    claim_geometry,
                ),
                Ok(_) => {
                    send_direct_error(
                        socket,
                        "resume_required",
                        "the first terminal message must establish a replay cursor and usable renderer size",
                    )
                    .await;
                    return None;
                }
                Err(error) => {
                    send_direct_error(socket, "invalid_message", &error.to_string()).await;
                    return None;
                }
            }
        }
        Ok(Some(Ok(_))) => {
            send_direct_error(
                socket,
                "resume_required",
                "the first terminal message must be a text replay request",
            )
            .await;
            return None;
        }
        Ok(Some(Err(_)) | None) => return None,
        Err(_) => {
            send_direct_error(
                socket,
                "resume_timeout",
                "the replay cursor was not provided within the bounded handshake",
            )
            .await;
            return None;
        }
    };

    // A terminal is one server-owned PTY with potentially many desktop and
    // mobile viewers. A selected foreground attachment may explicitly claim
    // its initial geometry; otherwise the requested size is retained by this
    // connection and becomes authoritative after this device sends input.
    Some((
        after_sequence,
        initial_size,
        owner_device_id,
        claim_geometry,
    ))
}

async fn stream_output(
    terminal_host: HostClient,
    session_id: WorkerSessionId,
    mut after_sequence: Option<u64>,
    outbound: mpsc::Sender<Message>,
) {
    let mut first_read = true;
    loop {
        let output = if first_read {
            first_read = false;
            read_output(&terminal_host, session_id, after_sequence).await
        } else {
            wait_for_output(&terminal_host, session_id, after_sequence).await
        };
        let (resume, running) = match output {
            Ok(output) => output,
            Err((code, message)) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: &code,
                        message,
                    },
                )
                .await;
                return;
            }
        };

        match resume {
            Resume::Deltas { frames } => {
                if send_output_frames(&outbound, frames, &mut after_sequence)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Resume::Snapshot { snapshot } => {
                if send_snapshot(&outbound, &snapshot).await.is_err() {
                    return;
                }
                after_sequence = Some(snapshot.sequence);
            }
        }

        if send_control(
            &outbound,
            &ServerTerminalMessage::State {
                running,
                latest_sequence: after_sequence.unwrap_or(0),
            },
        )
        .await
        .is_err()
        {
            return;
        }
        if !running {
            return;
        }
    }
}

async fn read_output(
    terminal_host: &HostClient,
    session_id: WorkerSessionId,
    after_sequence: Option<u64>,
) -> Result<(Resume, bool), (String, String)> {
    request_output(
        terminal_host,
        session_id,
        HostRequest::Read {
            session_id,
            after_sequence,
        },
    )
    .await
}

async fn wait_for_output(
    terminal_host: &HostClient,
    session_id: WorkerSessionId,
    after_sequence: Option<u64>,
) -> Result<(Resume, bool), (String, String)> {
    request_output(
        terminal_host,
        session_id,
        HostRequest::Wait {
            session_id,
            after_sequence,
        },
    )
    .await
}

async fn request_output(
    terminal_host: &HostClient,
    session_id: WorkerSessionId,
    request: HostRequest,
) -> Result<(Resume, bool), (String, String)> {
    match terminal_host.request(&request).await {
        Ok(HostResponse::Output {
            session_id: response_session,
            resume,
            running,
        }) if response_session == session_id => Ok((resume, running)),
        Ok(HostResponse::Error { code, message }) => Err((code, message)),
        Ok(_) => Err((
            "unexpected_host_response".into(),
            "terminal host returned an unexpected response".into(),
        )),
        Err(error) => Err(("terminal_host_unavailable".into(), error.to_string())),
    }
}

async fn send_output_frames(
    outbound: &mpsc::Sender<Message>,
    frames: Vec<swarm_terminal::SequencedFrame>,
    after_sequence: &mut Option<u64>,
) -> Result<(), ()> {
    for frame in frames {
        let expected = after_sequence.map_or(1, |sequence| sequence.saturating_add(1));
        if frame.sequence != expected {
            send_control(
                outbound,
                &ServerTerminalMessage::Error {
                    code: "terminal_sequence_gap",
                    message: format!(
                        "expected terminal sequence {}, received {}",
                        expected, frame.sequence
                    ),
                },
            )
            .await?;
            return Err(());
        }
        let mut payload = Vec::with_capacity(frame.bytes.len() + 9);
        payload.push(OUTPUT_FRAME_TYPE);
        payload.extend_from_slice(&frame.sequence.to_be_bytes());
        payload.extend_from_slice(&frame.bytes);
        outbound
            .send(Message::Binary(payload.into()))
            .await
            .map_err(|_| ())?;
        *after_sequence = Some(frame.sequence);
    }
    Ok(())
}

async fn send_snapshot(
    outbound: &mpsc::Sender<Message>,
    snapshot: &swarm_terminal::TerminalSnapshot,
) -> Result<(), ()> {
    let mut payload = Vec::with_capacity(snapshot.bytes.len() + 14);
    payload.push(SNAPSHOT_FRAME_TYPE);
    payload.extend_from_slice(&snapshot.sequence.to_be_bytes());
    payload.extend_from_slice(&snapshot.rows.to_be_bytes());
    payload.extend_from_slice(&snapshot.columns.to_be_bytes());
    payload.push(u8::from(snapshot.truncated));
    payload.extend_from_slice(&snapshot.bytes);
    outbound
        .send(Message::Binary(payload.into()))
        .await
        .map_err(|_| ())
}

struct TerminalInputContext {
    terminal_host: HostClient,
    session_id: WorkerSessionId,
    task_store: TaskStore,
    control_room_notify: Arc<Notify>,
    outbound: mpsc::Sender<Message>,
    owner_device_id: Option<PresenceDeviceId>,
}

async fn handle_input(
    mut socket_receiver: SplitStream<WebSocket>,
    context: TerminalInputContext,
    mut requested_size: TerminalSize,
) {
    let TerminalInputContext {
        terminal_host,
        session_id,
        task_store,
        control_room_notify,
        outbound,
        owner_device_id,
    } = context;
    while let Some(message) = socket_receiver.next().await {
        let message = match message {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) | Err(_) => return,
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Binary(_)) => continue,
        };
        let action = match client_request(&message, session_id, owner_device_id) {
            Ok(action) => action,
            Err(ClientMessageError {
                code,
                message,
                close,
            }) => {
                let _ =
                    send_control(&outbound, &ServerTerminalMessage::Error { code, message }).await;
                if close {
                    return;
                }
                continue;
            }
        };
        match action {
            ClientTerminalAction::Resize {
                size,
                claim_geometry,
            } => {
                requested_size = size;
                if !handle_resize(
                    &terminal_host,
                    &task_store,
                    session_id,
                    owner_device_id,
                    requested_size,
                    claim_geometry,
                    &outbound,
                )
                .await
                {
                    return;
                }
            }
            ClientTerminalAction::Input { request, engaged } => {
                if engaged {
                    if !record_operator_engagement(
                        &task_store,
                        session_id,
                        owner_device_id,
                        &control_room_notify,
                        &outbound,
                    )
                    .await
                    {
                        continue;
                    }
                    // Geometry follows the device that actually typed, before
                    // the provider receives that input. Other attached viewers
                    // continue rendering canonical output but cannot fight this
                    // size until they themselves become the engaged device.
                    if !forward_host_request(
                        &terminal_host,
                        HostRequest::Resize {
                            session_id,
                            size: requested_size,
                        },
                        &outbound,
                    )
                    .await
                    {
                        return;
                    }
                }
                if !forward_host_request(&terminal_host, request, &outbound).await {
                    return;
                }
            }
        }
    }
}

async fn handle_resize(
    terminal_host: &HostClient,
    task_store: &TaskStore,
    session_id: WorkerSessionId,
    owner_device_id: Option<PresenceDeviceId>,
    requested_size: TerminalSize,
    claim_geometry: bool,
    outbound: &mpsc::Sender<Message>,
) -> bool {
    let Some(owner_device_id) = owner_device_id else {
        let _ = send_control(
            outbound,
            &ServerTerminalMessage::Error {
                code: "terminal_resize_authority_unavailable",
                message: "terminal resize requires an identified device".into(),
            },
        )
        .await;
        return false;
    };
    let owns_geometry = if claim_geometry {
        task_store.claim_worker_geometry(session_id, owner_device_id)
    } else {
        task_store.claim_unowned_worker_geometry(session_id, owner_device_id)
    };
    let Ok(owns_geometry) = owns_geometry else {
        let _ = send_control(
            outbound,
            &ServerTerminalMessage::Error {
                code: "terminal_resize_authority_unavailable",
                message: "terminal resize authority could not be checked".into(),
            },
        )
        .await;
        return false;
    };
    !owns_geometry
        || forward_host_request(
            terminal_host,
            HostRequest::Resize {
                session_id,
                size: requested_size,
            },
            outbound,
        )
        .await
}

enum ClientTerminalAction {
    Input {
        request: HostRequest,
        engaged: bool,
    },
    Resize {
        size: TerminalSize,
        claim_geometry: bool,
    },
}

struct ClientMessageError {
    code: &'static str,
    message: String,
    close: bool,
}

fn client_request(
    message: &str,
    session_id: WorkerSessionId,
    owner_device_id: Option<PresenceDeviceId>,
) -> Result<ClientTerminalAction, ClientMessageError> {
    match serde_json::from_str::<ClientTerminalMessage>(message) {
        Ok(ClientTerminalMessage::Input { text }) => {
            let bytes = text.into_bytes();
            Ok(ClientTerminalAction::Input {
                engaged: !bytes.is_empty(),
                request: HostRequest::Write {
                    session_id,
                    provenance: TerminalWriteProvenance::operator(owner_device_id, &bytes),
                    bytes,
                },
            })
        }
        Ok(ClientTerminalMessage::Resize {
            rows,
            columns,
            claim_geometry,
        }) if rows >= MIN_TERMINAL_ROWS && columns >= MIN_TERMINAL_COLUMNS => {
            Ok(ClientTerminalAction::Resize {
                size: TerminalSize::new(rows, columns),
                claim_geometry,
            })
        }
        Ok(ClientTerminalMessage::Resize { .. }) => Err(ClientMessageError {
            code: "invalid_terminal_size",
            message: format!(
                "terminal dimensions must be at least {MIN_TERMINAL_ROWS} rows and {MIN_TERMINAL_COLUMNS} columns"
            ),
            close: false,
        }),
        Ok(ClientTerminalMessage::Resume { .. }) => Err(ClientMessageError {
            code: "duplicate_resume",
            message: "the replay cursor is immutable for an attachment".into(),
            close: true,
        }),
        Err(error) => Err(ClientMessageError {
            code: "invalid_message",
            message: error.to_string(),
            close: false,
        }),
    }
}

async fn forward_host_request(
    terminal_host: &HostClient,
    request: HostRequest,
    outbound: &mpsc::Sender<Message>,
) -> bool {
    match terminal_host.request(&request).await {
        Ok(HostResponse::Acknowledged) => true,
        Ok(HostResponse::Error { code, message }) => {
            let _ = send_control(
                outbound,
                &ServerTerminalMessage::Error {
                    code: &code,
                    message,
                },
            )
            .await;
            true
        }
        Ok(_) => {
            let _ = send_control(
                outbound,
                &ServerTerminalMessage::Error {
                    code: "unexpected_host_response",
                    message: "terminal host did not acknowledge the operation".into(),
                },
            )
            .await;
            true
        }
        Err(error) => {
            let _ = send_control(
                outbound,
                &ServerTerminalMessage::Error {
                    code: "terminal_host_unavailable",
                    message: error.to_string(),
                },
            )
            .await;
            false
        }
    }
}

async fn record_operator_engagement(
    task_store: &TaskStore,
    session_id: WorkerSessionId,
    owner_device_id: Option<PresenceDeviceId>,
    control_room_notify: &Notify,
    outbound: &mpsc::Sender<Message>,
) -> bool {
    let now = unix_timestamp();
    if let Ok(changed) = task_store.renew_worker_engagement(
        session_id,
        owner_device_id,
        now,
        OPERATOR_ENGAGEMENT_LEASE_SECONDS,
    ) {
        if changed {
            control_room_notify.notify_waiters();
        }
        return true;
    }
    let _ = send_control(
        outbound,
        &ServerTerminalMessage::Error {
            code: "engagement_unavailable",
            message: "operator engagement could not be recorded; input was not sent".into(),
        },
    )
    .await;
    false
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

async fn send_control(
    outbound: &mpsc::Sender<Message>,
    message: &ServerTerminalMessage<'_>,
) -> Result<(), ()> {
    let payload = serde_json::to_string(message).map_err(|_| ())?;
    outbound
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}

async fn send_direct_error(socket: &mut WebSocket, code: &str, message: &str) {
    if let Ok(payload) = serde_json::to_string(&ServerTerminalMessage::Error {
        code,
        message: message.into(),
    }) {
        let _ = socket.send(Message::Text(payload.into())).await;
    }
}
