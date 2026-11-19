//! Jira replies the operator posts from Needs you (ADR 0177).
//!
//! The press sends the reply in the request itself. It does not leave it for
//! the next Jira pass, which is up to a minute away, so the answer the card
//! gives is Jira's: delivered, or what Jira said.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_persistence::TaskStoreError;

use super::{
    ApiError, AppState, authorize, parse_task_id, task_store, task_store_error, unix_timestamp,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PostJiraReplyRequest {
    body: String,
    #[serde(default)]
    resolve: bool,
}

/// Where the press left the reply, and the resolution when one follows it.
#[derive(Debug, Serialize)]
struct PressAnswer {
    /// `delivered`, `queued` (Jira could not be reached; Swarm tries again by
    /// itself), `conflict` (Jira refused it) or `uncertain`.
    state: String,
    error: Option<String>,
    /// Set only when the ticket resolves once Jira has the reply: `delivered`,
    /// the transition's own state, or `waiting` while the reply has not
    /// reached Jira. None when no resolution follows this reply.
    resolution: Option<String>,
}

pub(super) async fn jira_replies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let replies = task_store(&state)?
        .jira_replies_awaiting()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(replies)).into_response())
}

pub(super) async fn post_jira_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<PostJiraReplyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    let posted = store
        .post_jira_reply(task_id, &request.body, request.resolve, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_jira_comments().await;
    let (delivery, error) = store
        .jira_comment_delivery(&posted.comment_id)
        .map_err(|error| task_store_error(&error))?
        .unwrap_or_else(|| ("queued".to_owned(), None));
    let resolution = if !posted.resolution_follows {
        None
    } else if delivery == "delivered" {
        // The resolution waited for this reply; it goes now, not a pass later.
        state.deliver_jira_transitions().await;
        Some(
            store
                .jira_transition_state_for_task(task_id)
                .map_err(|error| task_store_error(&error))?
                .unwrap_or_else(|| "delivered".to_owned()),
        )
    } else {
        Some("waiting".to_owned())
    };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(PressAnswer {
            state: delivery,
            error,
            resolution,
        }),
    )
        .into_response())
}

pub(super) async fn discard_jira_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let discarded = task_store(&state)?
        .discard_jira_reply(task_id)
        .map_err(|error| task_store_error(&error))?;
    if !discarded {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "jira_reply_not_drafted",
            TaskStoreError::JiraReplyNotDrafted.to_string(),
        ));
    }
    state.control_room_notify.notify_waiters();
    // A completion that waited for this reply can go to Jira now.
    state.deliver_jira_transitions().await;
    Ok(StatusCode::NO_CONTENT.into_response())
}
