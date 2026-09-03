//! Version-four browser adapter. The engine owns control; this adapter binds
//! authenticated attachments to one immutable view and never falls back on writes.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt, stream::SplitStream};
use serde::{Deserialize, Serialize};
use swarm_domain::{PresenceDeviceId, TerminalControlIdentity, TerminalViewId, WorkerSessionId};
use swarm_persistence::{TaskStore, TaskStoreError, TerminalControlProjection};
use swarm_terminal::{
    HostClient, HostRequest, HostResponse, MAX_CONTROL_INPUT_BYTES, MIN_TERMINAL_COLUMNS,
    MIN_TERMINAL_ROWS, PROTOCOL_VERSION, Resume, TERMINAL_CONTROL_PROTOCOL_VERSION,
    TerminalControlCommand, TerminalControlStatus, TerminalSize,
};
use tokio::sync::{Notify, mpsc};

use crate::terminal_socket::{send_output_frames, send_snapshot};

pub(super) const CONTROLLED_WEBSOCKET_PROTOCOL: &str = "swarm-terminal.v4";
type Failure = (String, String);

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ClientMessage {
    Probe {
        request_id: String,
    },
    Resume {
        after_sequence: Option<u64>,
        rows: u16,
        columns: u16,
        device_id: PresenceDeviceId,
        view_id: TerminalViewId,
        foreground: bool,
    },
    Claim {
        observed_generation: Option<String>,
        rows: u16,
        columns: u16,
    },
    Renew {
        generation: String,
    },
    Release {
        generation: String,
    },
    Input {
        generation: String,
        text: String,
    },
    Resize {
        generation: String,
        rows: u16,
        columns: u16,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage<'a> {
    Alive {
        request_id: &'a str,
    },
    Control {
        control: BrowserControl,
    },
    State {
        running: bool,
        latest_sequence: u64,
        control: BrowserControl,
    },
    Error {
        code: &'a str,
        message: &'a str,
    },
}

#[derive(Debug, Serialize)]
struct BrowserControl {
    supported: bool,
    /// Decimal string: a u64 generation must never be rounded by JavaScript.
    generation: Option<String>,
    owned: bool,
    occupied: bool,
    lease_remaining_ms: u64,
}

impl BrowserControl {
    fn from_engine(
        status: Option<TerminalControlStatus>,
        identity: TerminalControlIdentity,
    ) -> Self {
        Self {
            supported: status.is_some(),
            generation: status.map(|value| value.generation.to_string()),
            owned: status.is_some_and(|value| value.owner == Some(identity)),
            occupied: status.is_some_and(|value| value.owner.is_some()),
            lease_remaining_ms: status.map_or(0, |value| value.lease_remaining_ms),
        }
    }
}

pub(super) async fn engine_supports_control(client: &HostClient) -> Result<bool, Failure> {
    match request(client, &HostRequest::Ping).await? {
        HostResponse::Pong { protocol_version } => {
            Ok((TERMINAL_CONTROL_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&protocol_version))
        }
        _ => Err(unexpected()),
    }
}

async fn request(client: &HostClient, request: &HostRequest) -> Result<HostResponse, Failure> {
    // WaitControlled has a host-side 30-second bound. Every other operation uses
    // a short transport bound. A timed-out input is uncertain, never replayed.
    let timeout = if matches!(
        request,
        HostRequest::WaitControlled { .. } | HostRequest::Wait { .. }
    ) {
        Duration::from_secs(35)
    } else {
        Duration::from_secs(5)
    };
    match tokio::time::timeout(timeout, client.request(request)).await {
        Ok(Ok(HostResponse::Error { code, message })) => Err((code, message)),
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(("terminal_host_unavailable".into(), error.to_string())),
        Err(_) => Err((
            "terminal_host_timeout".into(),
            "The worker engine did not confirm this operation; input will not be retried.".into(),
        )),
    }
}

fn unexpected() -> Failure {
    (
        "unexpected_host_response".into(),
        "The worker engine returned an unexpected response.".into(),
    )
}

async fn control_request(
    client: &HostClient,
    session_id: WorkerSessionId,
    command: TerminalControlCommand,
) -> Result<TerminalControlStatus, Failure> {
    match request(
        client,
        &HostRequest::Control {
            session_id,
            command,
        },
    )
    .await?
    {
        HostResponse::Control {
            session_id: returned,
            control,
        } if returned == session_id => Ok(control),
        _ => Err(unexpected()),
    }
}

fn size(rows: u16, columns: u16) -> Result<TerminalSize, Failure> {
    if rows < MIN_TERMINAL_ROWS || columns < MIN_TERMINAL_COLUMNS {
        return Err((
            "invalid_terminal_size".into(),
            "The terminal needs usable rows and columns.".into(),
        ));
    }
    Ok(TerminalSize::new(rows, columns))
}

fn generation(value: &str) -> Result<u64, Failure> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err((
            "invalid_control_generation".into(),
            "A decimal control generation is required.".into(),
        ));
    }
    value.parse().map_err(|_| {
        (
            "invalid_control_generation".into(),
            "The control generation is out of range.".into(),
        )
    })
}

fn command(
    message: ClientMessage,
    identity: TerminalControlIdentity,
) -> Result<TerminalControlCommand, Failure> {
    Ok(match message {
        ClientMessage::Probe { .. } => {
            return Err((
                "invalid_control_command".into(),
                "A probe is not an engine command.".into(),
            ));
        }
        ClientMessage::Claim {
            observed_generation,
            rows,
            columns,
        } => TerminalControlCommand::Claim {
            identity,
            observed_generation: observed_generation.as_deref().map(generation).transpose()?,
            size: size(rows, columns)?,
        },
        ClientMessage::Renew { generation: value } => TerminalControlCommand::Renew {
            identity,
            generation: generation(&value)?,
        },
        ClientMessage::Release { generation: value } => TerminalControlCommand::Release {
            identity,
            generation: generation(&value)?,
        },
        ClientMessage::Resize {
            generation: value,
            rows,
            columns,
        } => TerminalControlCommand::Resize {
            identity,
            generation: generation(&value)?,
            size: size(rows, columns)?,
        },
        ClientMessage::Input {
            generation: value,
            text,
        } => {
            if text.is_empty() || text.len() > MAX_CONTROL_INPUT_BYTES {
                return Err((
                    "terminal_input_invalid".into(),
                    "Terminal input must be nonempty and bounded.".into(),
                ));
            }
            TerminalControlCommand::Input {
                identity,
                generation: generation(&value)?,
                bytes: text.into_bytes(),
            }
        }
        ClientMessage::Resume { .. } => {
            return Err((
                "duplicate_resume".into(),
                "The attachment identity and replay cursor cannot change.".into(),
            ));
        }
    })
}

enum ClientAction {
    Probe(String),
    Control(TerminalControlCommand),
}

fn action(text: &str, identity: TerminalControlIdentity) -> Result<ClientAction, Failure> {
    let message = serde_json::from_str::<ClientMessage>(text).map_err(|_| {
        (
            "invalid_message".into(),
            "Invalid terminal control message.".into(),
        )
    })?;
    if let ClientMessage::Probe { request_id } = message {
        if request_id.is_empty() || request_id.len() > 64 {
            return Err((
                "invalid_probe".into(),
                "A bounded probe identifier is required.".into(),
            ));
        }
        return Ok(ClientAction::Probe(request_id));
    }
    command(message, identity).map(ClientAction::Control)
}

async fn send(outbound: &mpsc::Sender<Message>, message: &ServerMessage<'_>) -> Result<(), ()> {
    let text = serde_json::to_string(message).map_err(|_| ())?;
    outbound
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn send_failure(outbound: &mpsc::Sender<Message>, failure: &Failure) -> Result<(), ()> {
    send(
        outbound,
        &ServerMessage::Error {
            code: &failure.0,
            message: &failure.1,
        },
    )
    .await
}

struct Handshake {
    after_sequence: Option<u64>,
    size: TerminalSize,
    identity: TerminalControlIdentity,
    foreground: bool,
}

async fn handshake(socket: &mut WebSocket) -> Option<Handshake> {
    let initial = tokio::time::timeout(Duration::from_secs(5), socket.recv()).await;
    let Ok(Some(Ok(Message::Text(text)))) = initial else {
        return None;
    };
    let Ok(ClientMessage::Resume {
        after_sequence,
        rows,
        columns,
        device_id,
        view_id,
        foreground,
    }) = serde_json::from_str(&text)
    else {
        return None;
    };
    let Ok(initial_size) = size(rows, columns) else {
        return None;
    };
    let identity = TerminalControlIdentity {
        device: device_id,
        view: view_id,
    };
    Some(Handshake {
        after_sequence,
        size: initial_size,
        identity,
        foreground,
    })
}

pub(super) async fn serve(
    mut socket: WebSocket,
    client: HostClient,
    session_id: WorkerSessionId,
    store: TaskStore,
    notify: Arc<Notify>,
    supported: bool,
) {
    let Some(Handshake {
        after_sequence,
        size: initial_size,
        identity,
        foreground,
    }) = handshake(&mut socket).await
    else {
        return;
    };
    let (mut sink, receiver) = socket.split();
    let (outbound, mut messages) = mpsc::channel::<Message>(16);
    let mut writer = tokio::spawn(async move {
        while let Some(message) = messages.recv().await {
            if !matches!(
                tokio::time::timeout(Duration::from_secs(10), sink.send(message)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    });

    let context = SocketContext {
        client,
        session_id,
        identity,
        store,
        notify,
        supported,
        outbound: outbound.clone(),
    };
    let mut initial_control = None;
    if supported {
        let initial_command = if foreground {
            TerminalControlCommand::Claim {
                identity,
                observed_generation: None,
                size: initial_size,
            }
        } else {
            TerminalControlCommand::Status
        };
        match control_request(&context.client, session_id, initial_command).await {
            Ok(status) => {
                context.project_engagement(status).await;
                initial_control = Some(status);
            }
            Err(failure)
                if matches!(
                    failure.0.as_str(),
                    "terminal_control_owned_elsewhere" | "terminal_takeover_active"
                ) =>
            {
                // A passive viewer is normal, not an error or permission to steal.
                if let Ok(status) =
                    control_request(&context.client, session_id, TerminalControlCommand::Status)
                        .await
                {
                    context.project_engagement(status).await;
                    initial_control = Some(status);
                }
            }
            Err(failure) => {
                let _ = send_failure(&outbound, &failure).await;
            }
        }
        if initial_control.is_none() {
            drop(context);
            drop(outbound);
            finish_writer(writer).await;
            return;
        }
    }
    let output_context = context.clone();
    let mut output = tokio::spawn(async move {
        output_context.stream(after_sequence, initial_control).await;
    });
    let mut input = tokio::spawn(async move {
        context.input(receiver).await;
    });
    // Any completed half owns cancellation of the others. No orphaned socket
    // task or wait survives attachment teardown; engine sessions remain alive.
    let finished =
        tokio::select! { _ = &mut writer => 0, _ = &mut output => 1, _ = &mut input => 2 };
    if finished != 1 {
        output.abort();
        let _ = output.await;
    }
    if finished != 2 {
        input.abort();
        let _ = input.await;
    }
    drop(outbound);
    if finished != 0 {
        finish_writer(writer).await;
    }
}

async fn finish_writer(mut writer: tokio::task::JoinHandle<()>) {
    if tokio::time::timeout(Duration::from_secs(10), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
        let _ = writer.await;
    }
}

#[derive(Clone)]
struct SocketContext {
    client: HostClient,
    session_id: WorkerSessionId,
    identity: TerminalControlIdentity,
    store: TaskStore,
    notify: Arc<Notify>,
    supported: bool,
    outbound: mpsc::Sender<Message>,
}

#[derive(Clone, Copy)]
struct ProjectionCheckpoint {
    generation: u64,
    owner: Option<TerminalControlIdentity>,
    deadline: Instant,
}

impl ProjectionCheckpoint {
    fn observe(status: TerminalControlStatus, now: Instant) -> Self {
        Self {
            generation: status.generation,
            owner: status.owner,
            deadline: now + Duration::from_millis(status.lease_remaining_ms.min(300_000)),
        }
    }

    fn changed_enough(self, previous: Option<Self>) -> bool {
        previous.is_none_or(|previous| {
            self.generation != previous.generation
                || self.owner != previous.owner
                || self.deadline > previous.deadline + Duration::from_secs(30)
        })
    }
}

impl SocketContext {
    async fn project_engagement(&self, status: TerminalControlStatus) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |value| {
                i64::try_from(value.as_secs()).unwrap_or(i64::MAX)
            });
        match self.store.project_terminal_control(
            TerminalControlProjection {
                session_id: self.session_id,
                generation: status.generation,
                owner: status.owner,
                lease_remaining_ms: status.lease_remaining_ms,
            },
            now,
        ) {
            Ok(true) => {
                self.notify.notify_waiters();
                true
            }
            Ok(false) | Err(TaskStoreError::WorkerSessionNotActive) => true,
            Err(_) => {
                // The PTY effect already happened. Never imply input was not sent.
                let _ = send_failure(&self.outbound, &("engagement_projection_unavailable".into(), "Terminal control succeeded, but the activity indicator could not be updated.".into())).await;
                false
            }
        }
    }

    async fn input(self, mut receiver: SplitStream<WebSocket>) {
        let mut projected = None;
        while let Some(message) = receiver.next().await {
            let text = match message {
                Ok(Message::Text(text)) => text,
                Ok(Message::Close(_)) | Err(_) => return,
                _ => continue,
            };
            let command = match action(&text, self.identity) {
                Ok(ClientAction::Probe(request_id)) => {
                    // Transport liveness is independent of engine capabilities.
                    // This must neither renew ownership nor copy terminal state.
                    if send(
                        &self.outbound,
                        &ServerMessage::Alive {
                            request_id: &request_id,
                        },
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                    continue;
                }
                Ok(ClientAction::Control(command)) => command,
                Err(failure) => {
                    let _ = send_failure(&self.outbound, &failure).await;
                    if failure.0 == "duplicate_resume" {
                        return;
                    }
                    continue;
                }
            };
            if !self.supported {
                let _ = send_failure(&self.outbound, &("terminal_engine_update_required".into(), "This worker engine supports viewing only with this client. Update the engine safely to enable Resume Here.".into())).await;
                continue;
            }
            match control_request(&self.client, self.session_id, command).await {
                Ok(status) => {
                    // A typing burst must not perform a SQLite read per key.
                    // This only coalesces the indicator; the engine still checks
                    // every operation and persists no authority in this cache.
                    let checkpoint = ProjectionCheckpoint::observe(status, Instant::now());
                    if checkpoint.changed_enough(projected) && self.project_engagement(status).await
                    {
                        projected = Some(checkpoint);
                    }
                    if send(
                        &self.outbound,
                        &ServerMessage::Control {
                            control: BrowserControl::from_engine(Some(status), self.identity),
                        },
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                Err(failure) => {
                    if send_failure(&self.outbound, &failure).await.is_err() {
                        return;
                    }
                    // Authority refusals refresh ownership, never repeat an input.
                    if failure.0.starts_with("terminal_control_") {
                        if let Ok(status) = control_request(
                            &self.client,
                            self.session_id,
                            TerminalControlCommand::Status,
                        )
                        .await
                        {
                            self.project_engagement(status).await;
                            let _ = send(
                                &self.outbound,
                                &ServerMessage::Control {
                                    control: BrowserControl::from_engine(
                                        Some(status),
                                        self.identity,
                                    ),
                                },
                            )
                            .await;
                        }
                    } else if matches!(
                        failure.0.as_str(),
                        "terminal_host_unavailable" | "terminal_host_timeout"
                    ) {
                        return;
                    }
                }
            }
        }
    }

    async fn stream(
        &self,
        mut after_sequence: Option<u64>,
        mut control: Option<TerminalControlStatus>,
    ) {
        let mut first = true;
        loop {
            let request_value = if first {
                first = false;
                HostRequest::Read {
                    session_id: self.session_id,
                    after_sequence,
                }
            } else if let Some(status) = control {
                HostRequest::WaitControlled {
                    session_id: self.session_id,
                    after_sequence,
                    after_control: status.cursor(),
                }
            } else {
                // Explicit read-only compatibility. Never send legacy Write/Resize.
                HostRequest::Wait {
                    session_id: self.session_id,
                    after_sequence,
                }
            };
            let result = request(&self.client, &request_value).await;
            let (resume, running) = match result {
                Ok(HostResponse::Output {
                    session_id,
                    resume,
                    running,
                }) if session_id == self.session_id => (resume, running),
                Ok(HostResponse::ControlledOutput {
                    session_id,
                    resume,
                    running,
                    control: status,
                }) if session_id == self.session_id => {
                    if control.is_none_or(|previous| previous.cursor() != status.cursor()) {
                        self.project_engagement(status).await;
                    }
                    control = Some(status);
                    (resume, running)
                }
                other => {
                    let failure = other.err().unwrap_or_else(unexpected);
                    let _ = send_failure(&self.outbound, &failure).await;
                    return;
                }
            };
            let sent = match resume {
                Resume::Deltas { frames } => {
                    send_output_frames(&self.outbound, frames, &mut after_sequence).await
                }
                Resume::Snapshot { snapshot } => {
                    after_sequence = Some(snapshot.sequence);
                    send_snapshot(&self.outbound, &snapshot).await
                }
            };
            if sent.is_err() {
                return;
            }
            if send(
                &self.outbound,
                &ServerMessage::State {
                    running,
                    latest_sequence: after_sequence.unwrap_or(0),
                    control: BrowserControl::from_engine(control, self.identity),
                },
            )
            .await
            .is_err()
                || !running
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> TerminalControlIdentity {
        TerminalControlIdentity {
            device: PresenceDeviceId::new(),
            view: TerminalViewId::new(),
        }
    }

    #[test]
    fn probes_are_bounded_transport_actions_not_engine_commands() {
        assert!(
            matches!(action(r#"{"type":"probe","request_id":"return-1"}"#, identity()).unwrap(), ClientAction::Probe(id) if id == "return-1")
        );
        for request_id in [String::new(), "x".repeat(65)] {
            let text = serde_json::json!({ "type": "probe", "request_id": request_id }).to_string();
            assert!(action(&text, identity()).is_err());
        }
        assert!(
            action(
                r#"{"type":"probe","request_id":"x","generation":"1"}"#,
                identity()
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(ServerMessage::Alive {
                request_id: "return-1"
            })
            .unwrap(),
            serde_json::json!({ "type": "alive", "request_id": "return-1" })
        );
    }

    #[test]
    fn generations_preserve_the_full_engine_range_and_reject_coercions() {
        assert_eq!(generation("18446744073709551615").unwrap(), u64::MAX);
        for value in ["", "-1", "+1", "1.0", "1e2", " 1", "18446744073709551616"] {
            assert!(generation(value).is_err(), "{value}");
        }
        let owner = identity();
        let control = BrowserControl::from_engine(
            Some(TerminalControlStatus {
                generation: u64::MAX,
                owner: Some(owner),
                lease_remaining_ms: 100,
            }),
            owner,
        );
        let json = serde_json::to_value(control).unwrap();
        assert_eq!(json["generation"], "18446744073709551615");
        assert_eq!(json["owned"], true);
    }

    #[test]
    fn input_is_bound_to_handshake_identity_and_never_a_legacy_write() {
        let owner = identity();
        let parsed =
            serde_json::from_str(r#"{"type":"input","generation":"7","text":"hello"}"#).unwrap();
        assert!(
            matches!(command(parsed, owner).unwrap(), TerminalControlCommand::Input { identity, generation: 7, bytes } if identity == owner && bytes == b"hello")
        );
        for text in [
            r#"{"type":"input","generation":7,"text":"hello"}"#,
            r#"{"type":"input","generation":"7","text":"hello","view_id":"other"}"#,
        ] {
            assert!(serde_json::from_str::<ClientMessage>(text).is_err());
        }
        assert!(
            command(
                ClientMessage::Input {
                    generation: "7".into(),
                    text: String::new()
                },
                owner
            )
            .is_err()
        );
        assert!(
            command(
                ClientMessage::Input {
                    generation: "7".into(),
                    text: "x".repeat(MAX_CONTROL_INPUT_BYTES + 1)
                },
                owner
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_engines_and_other_views_are_passive_not_implicitly_owned() {
        let owner = identity();
        assert!(!BrowserControl::from_engine(None, owner).supported);
        assert!(!BrowserControl::from_engine(None, owner).owned);
        let other = TerminalControlIdentity {
            device: owner.device,
            view: TerminalViewId::new(),
        };
        let status = TerminalControlStatus {
            generation: 1,
            owner: Some(owner),
            lease_remaining_ms: 90_000,
        };
        let passive = BrowserControl::from_engine(Some(status), other);
        assert!(passive.supported && passive.occupied && !passive.owned);
        assert!(
            command(
                ClientMessage::Resize {
                    generation: "1".into(),
                    rows: 0,
                    columns: 80
                },
                owner
            )
            .is_err()
        );
    }

    #[test]
    fn typing_projection_is_coalesced_but_handoff_and_expiry_are_not() {
        let now = Instant::now();
        let mut status = TerminalControlStatus {
            generation: 1,
            owner: Some(identity()),
            lease_remaining_ms: 90_000,
        };
        let previous = ProjectionCheckpoint::observe(status, now);
        assert!(
            !ProjectionCheckpoint::observe(status, now + Duration::from_secs(1))
                .changed_enough(Some(previous))
        );
        status.lease_remaining_ms = 300_000;
        assert!(ProjectionCheckpoint::observe(status, now).changed_enough(Some(previous)));
        status.owner = None;
        assert!(ProjectionCheckpoint::observe(status, now).changed_enough(Some(previous)));
        status.owner = previous.owner;
        status.generation = 2;
        assert!(ProjectionCheckpoint::observe(status, now).changed_enough(Some(previous)));
    }
}
