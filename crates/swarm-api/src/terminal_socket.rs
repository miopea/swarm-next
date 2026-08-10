use futures_util::{SinkExt, StreamExt, stream::SplitStream};
use serde::{Deserialize, Serialize};
use swarm_domain::WorkerSessionId;
use swarm_terminal::{HostClient, HostRequest, HostResponse, Resume, TerminalSize};
use tokio::sync::mpsc;

use axum::extract::ws::{Message, WebSocket};

pub const TERMINAL_WEBSOCKET_PROTOCOL: &str = "swarm-terminal.v3";
pub const TERMINAL_GRANT_PROTOCOL_PREFIX: &str = "swarm-grant.";
pub const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 64 * 1024;
const OUTBOUND_MESSAGE_CAPACITY: usize = 64;
const OUTPUT_FRAME_TYPE: u8 = 1;
const SNAPSHOT_FRAME_TYPE: u8 = 2;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientTerminalMessage {
    Resume {
        after_sequence: Option<u64>,
        rows: u16,
        columns: u16,
    },
    Input {
        text: String,
    },
    Resize {
        rows: u16,
        columns: u16,
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
) {
    let Some(after_sequence) =
        complete_initial_handshake(&mut socket, &terminal_host, session_id).await
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

    let output_host = terminal_host.clone();
    let output_sender = outbound_sender.clone();
    let mut output = tokio::spawn(async move {
        stream_output(output_host, session_id, after_sequence, output_sender).await;
    });
    let mut input = tokio::spawn(handle_input(
        socket_receiver,
        terminal_host,
        session_id,
        outbound_sender.clone(),
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
    terminal_host: &HostClient,
    session_id: WorkerSessionId,
) -> Option<Option<u64>> {
    let initial = tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv()).await;
    let (after_sequence, initial_size) = match initial {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<ClientTerminalMessage>(&text) {
                Ok(ClientTerminalMessage::Resume {
                    after_sequence,
                    rows,
                    columns,
                }) if rows > 0 && columns > 0 => (after_sequence, TerminalSize::new(rows, columns)),
                Ok(_) => {
                    send_direct_error(
                        socket,
                        "resume_required",
                        "the first terminal message must establish a replay cursor and non-zero renderer size",
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

    match terminal_host
        .request(&HostRequest::Resize {
            session_id,
            size: initial_size,
        })
        .await
    {
        Ok(HostResponse::Acknowledged) => {}
        Ok(HostResponse::Error { code, message }) => {
            send_direct_error(socket, &code, &message).await;
            return None;
        }
        Ok(_) => {
            send_direct_error(
                socket,
                "unexpected_host_response",
                "terminal host did not acknowledge the initial renderer size",
            )
            .await;
            return None;
        }
        Err(error) => {
            send_direct_error(socket, "terminal_host_unavailable", &error.to_string()).await;
            return None;
        }
    }
    Some(after_sequence)
}

async fn stream_output(
    terminal_host: HostClient,
    session_id: WorkerSessionId,
    mut after_sequence: Option<u64>,
    outbound: mpsc::Sender<Message>,
) {
    loop {
        let (resume, running) =
            match wait_for_output(&terminal_host, session_id, after_sequence).await {
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

async fn wait_for_output(
    terminal_host: &HostClient,
    session_id: WorkerSessionId,
    after_sequence: Option<u64>,
) -> Result<(Resume, bool), (String, String)> {
    match terminal_host
        .request(&HostRequest::Wait {
            session_id,
            after_sequence,
        })
        .await
    {
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

async fn handle_input(
    mut socket_receiver: SplitStream<WebSocket>,
    terminal_host: HostClient,
    session_id: WorkerSessionId,
    outbound: mpsc::Sender<Message>,
) {
    while let Some(message) = socket_receiver.next().await {
        let message = match message {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) | Err(_) => return,
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Binary(_)) => continue,
        };
        let request = match serde_json::from_str::<ClientTerminalMessage>(&message) {
            Ok(ClientTerminalMessage::Input { text }) => HostRequest::Write {
                session_id,
                bytes: text.into_bytes(),
            },
            Ok(ClientTerminalMessage::Resize { rows, columns }) if rows > 0 && columns > 0 => {
                HostRequest::Resize {
                    session_id,
                    size: swarm_terminal::TerminalSize::new(rows, columns),
                }
            }
            Ok(ClientTerminalMessage::Resize { .. }) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: "invalid_terminal_size",
                        message: "terminal dimensions must be non-zero".into(),
                    },
                )
                .await;
                continue;
            }
            Ok(ClientTerminalMessage::Resume { .. }) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: "duplicate_resume",
                        message: "the replay cursor is immutable for an attachment".into(),
                    },
                )
                .await;
                return;
            }
            Err(error) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: "invalid_message",
                        message: error.to_string(),
                    },
                )
                .await;
                continue;
            }
        };
        match terminal_host.request(&request).await {
            Ok(HostResponse::Acknowledged) => {}
            Ok(HostResponse::Error { code, message }) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: &code,
                        message,
                    },
                )
                .await;
            }
            Ok(_) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: "unexpected_host_response",
                        message: "terminal host did not acknowledge the operation".into(),
                    },
                )
                .await;
            }
            Err(error) => {
                let _ = send_control(
                    &outbound,
                    &ServerTerminalMessage::Error {
                        code: "terminal_host_unavailable",
                        message: error.to_string(),
                    },
                )
                .await;
                return;
            }
        }
    }
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
