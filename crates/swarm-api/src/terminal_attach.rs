use std::{str::FromStr, sync::Arc};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::PresenceDeviceId;
use swarm_terminal::{HostRequest, HostResponse, TerminalControlCommand};

use crate::{
    ApiError, AppState,
    attach::{ATTACH_GRANT_TTL, AttachGrantError, AttachProtocol},
    attachment_error, authorize, host_unavailable, parse_session_id, task_store, task_store_error,
    terminal_control_socket::{self, CONTROLLED_WEBSOCKET_PROTOCOL},
    terminal_host::{request_host, terminal_client},
    terminal_socket::{
        MAX_WEBSOCKET_MESSAGE_BYTES, TERMINAL_GRANT_PROTOCOL_PREFIX, TERMINAL_WEBSOCKET_PROTOCOL,
        serve_terminal_socket,
    },
};

#[derive(Debug, Serialize)]
struct AttachGrantResponse {
    grant: String,
    protocol: &'static str,
    websocket_path: String,
    expires_in_ms: u64,
}

#[derive(Serialize)]
struct TerminalAttachmentResponse {
    path: String,
}

#[derive(Default, Deserialize)]
pub(super) struct AttachQuery {
    protocol: Option<String>,
}

fn requested_protocol(value: Option<&str>) -> Result<AttachProtocol, ApiError> {
    match value {
        None | Some(TERMINAL_WEBSOCKET_PROTOCOL) => Ok(AttachProtocol::Legacy),
        Some(CONTROLLED_WEBSOCKET_PROTOCOL) => Ok(AttachProtocol::Controlled),
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_terminal_protocol",
            "This terminal protocol is not supported.",
        )),
    }
}

fn protocol_name(protocol: AttachProtocol) -> &'static str {
    match protocol {
        AttachProtocol::Legacy => TERMINAL_WEBSOCKET_PROTOCOL,
        AttachProtocol::Controlled => CONTROLLED_WEBSOCKET_PROTOCOL,
    }
}

async fn supports_control(client: &swarm_terminal::HostClient) -> Result<bool, ApiError> {
    terminal_control_socket::engine_supports_control(client)
        .await
        .map_err(|(_, message)| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "terminal_host_unavailable",
                message,
            )
        })
}

pub(super) async fn issue_grant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<AttachQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let client = terminal_client(&state)?;
    let protocol = requested_protocol(query.protocol.as_deref())?;
    let controlled = protocol == AttachProtocol::Controlled && supports_control(client).await?;
    let validation = if controlled {
        // A status read confirms identity without copying the terminal journal.
        HostRequest::Control {
            session_id,
            command: TerminalControlCommand::Status,
        }
    } else {
        HostRequest::Read {
            session_id,
            after_sequence: Some(u64::MAX),
        }
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.request(&validation),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminal_host_timeout",
            "The worker engine did not confirm this session in time.",
        )
    })?
    .map_err(|error| host_unavailable(&error))?
    {
        HostResponse::Output {
            session_id: returned,
            ..
        } if !controlled && returned == session_id => {}
        HostResponse::Control {
            session_id: returned,
            ..
        } if controlled && returned == session_id => {}
        HostResponse::Error { message, .. } => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "terminal_operation_failed",
                message,
            ));
        }
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "unexpected_host_response",
                "terminal host returned an unexpected response",
            ));
        }
    }
    let grant = state
        .attach_grants
        .issue_for(session_id, protocol)
        .map_err(|error| attach_grant_error(&error))?;
    let response = AttachGrantResponse {
        grant,
        protocol: protocol_name(protocol),
        websocket_path: format!("/api/v1/terminal/sessions/{session_id}/attach"),
        expires_in_ms: u64::try_from(ATTACH_GRANT_TTL.as_millis()).unwrap_or(u64::MAX),
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

pub(super) async fn attach(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let session_id = parse_session_id(&session_id)?;
    let grant = websocket_grant(&headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "attach_grant_required",
            "a short-lived terminal attach grant is required",
        )
    })?;
    let protocol = offered_protocol(&headers)?;
    let permit = Arc::clone(&state.websocket_limit)
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "terminal_websocket_limit_reached",
                "terminal WebSocket capacity is exhausted",
            )
        })?;
    if !state.attach_grants.consume_for(grant, session_id, protocol) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_attach_grant",
            "the terminal attach grant is invalid, expired, or already used",
        ));
    }
    let client = terminal_client(&state)?.clone();
    // Recheck on attachment: an engine can change between grant and upgrade.
    let controlled = protocol == AttachProtocol::Controlled && supports_control(&client).await?;
    let store = task_store(&state)?.clone();
    let control_room_notify = Arc::clone(&state.control_room_notify);
    Ok(websocket
        .protocols([protocol_name(protocol)])
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .max_frame_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            if protocol == AttachProtocol::Controlled {
                terminal_control_socket::serve(
                    socket,
                    client,
                    session_id,
                    store,
                    control_room_notify,
                    controlled,
                )
                .await;
            } else {
                serve_terminal_socket(socket, client, session_id, store, control_room_notify).await;
            }
        }))
}

pub(super) async fn release_engagement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((session_id, device_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "device_id must be a UUID",
        )
    })?;
    let released = task_store(&state)?
        .release_worker_engagement(session_id, device_id)
        .map_err(|error| task_store_error(&error))?;
    if released {
        state.control_room_notify.notify_waiters();
        state.deliver_coordination().await;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn upload_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let HostResponse::Sessions { sessions } =
        request_host(&state, HostRequest::ListSessions).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "terminal_protocol_error",
            "terminal host returned an unexpected response",
        ));
    };
    if !sessions
        .iter()
        .any(|session| session.session_id == session_id)
    {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "terminal_session_not_found",
            "terminal session does not exist",
        ));
    }
    let path = store
        .save(media_type, &body)
        .await
        .map_err(attachment_error)?;
    let path = path.to_str().ok_or_else(|| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attachment_path_unavailable",
            "private attachment path is not valid UTF-8",
        )
    })?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(TerminalAttachmentResponse { path: path.into() }),
    )
        .into_response())
}

fn attach_grant_error(error: &AttachGrantError) -> ApiError {
    match error {
        AttachGrantError::CapacityReached { .. } => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "attach_grant_limit_reached",
            error.to_string(),
        ),
        AttachGrantError::RandomnessUnavailable | AttachGrantError::LockPoisoned => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attach_grant_unavailable",
            error.to_string(),
        ),
    }
}

fn websocket_grant(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix(TERMINAL_GRANT_PROTOCOL_PREFIX))
}

fn offered_protocol(headers: &HeaderMap) -> Result<AttachProtocol, ApiError> {
    let protocols = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    // Prefer the stronger contract when both are offered. The grant must match;
    // do not consume a controlled grant and silently negotiate legacy input.
    if protocols
        .split(',')
        .map(str::trim)
        .any(|value| value == CONTROLLED_WEBSOCKET_PROTOCOL)
    {
        return Ok(AttachProtocol::Controlled);
    }
    if protocols
        .split(',')
        .map(str::trim)
        .any(|value| value == TERMINAL_WEBSOCKET_PROTOCOL)
    {
        return Ok(AttachProtocol::Legacy);
    }
    Err(ApiError::new(
        StatusCode::BAD_REQUEST,
        "unsupported_terminal_protocol",
        "Offer a supported terminal WebSocket protocol.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_negotiation_never_silently_downgrades() {
        assert_eq!(requested_protocol(None).unwrap(), AttachProtocol::Legacy);
        assert_eq!(
            requested_protocol(Some(CONTROLLED_WEBSOCKET_PROTOCOL)).unwrap(),
            AttachProtocol::Controlled
        );
        assert!(requested_protocol(Some("swarm-terminal.v999")).is_err());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            "swarm-terminal.v3, swarm-grant.test, swarm-terminal.v4"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            offered_protocol(&headers).unwrap(),
            AttachProtocol::Controlled
        );
        assert_eq!(websocket_grant(&headers), Some("test"));
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            "swarm-grant.test".parse().unwrap(),
        );
        assert!(offered_protocol(&headers).is_err());
    }
}
