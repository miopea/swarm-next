use std::{collections::HashSet, path::PathBuf};

use axum::http::StatusCode;
use swarm_domain::{ProviderKind, WorkerId, WorkerSessionId};
use swarm_terminal::{
    ClaudeConversationStart, CodexConversationStart, HostRequest, HostResponse, ProviderActivity,
    TerminalSize,
};

use crate::{
    ApiError, AppState, task_store, task_store_error, terminal_host::request_host, worker_view,
};

pub(super) async fn start_worker_process(
    state: &AppState,
    worker_id: WorkerId,
    size: TerminalSize,
) -> Result<crate::WorkerView, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    let live = reconcile_worker_bindings_unlocked(state).await?;
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    if let Some(session_id) = profile.active_session_id
        && live.contains(&session_id)
    {
        return Ok(worker_view(
            profile,
            true,
            false,
            None,
            ProviderActivity::Unknown,
        ));
    }
    let mcp_config = if profile.provider == ProviderKind::ClaudeCode {
        state
            .agent_bridge
            .as_ref()
            .map(|bridge| bridge.ensure_worker_config(worker_id))
            .transpose()
            .map_err(|error| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "agent_config_unavailable",
                    error.to_string(),
                )
            })?
    } else {
        None
    };

    let worker_workspace = PathBuf::from(&profile.workspace);
    let allow_outside_roots = !state
        .workspace_roots
        .iter()
        .any(|root| worker_workspace.starts_with(root));
    let request = match profile.provider {
        ProviderKind::ClaudeCode => HostRequest::StartClaude {
            workspace: worker_workspace.clone(),
            size,
            conversation: match (
                profile.provider_conversation_id,
                profile.has_session_history,
            ) {
                (Some(session_id), false) => ClaudeConversationStart::New { session_id },
                (Some(session_id), true) => ClaudeConversationStart::Resume { session_id },
                (None, true) => ClaudeConversationStart::Continue,
                (None, false) => {
                    let session_id = task_store(state)?
                        .assign_provider_conversation(worker_id)
                        .map_err(|error| task_store_error(&error))?;
                    ClaudeConversationStart::New { session_id }
                }
            },
            mcp_config,
            allow_outside_roots,
        },
        ProviderKind::Codex => HostRequest::StartCodex {
            workspace: worker_workspace,
            size,
            conversation: if profile.has_session_history {
                CodexConversationStart::Continue
            } else {
                CodexConversationStart::New
            },
            allow_outside_roots,
        },
    };
    let response = request_host(state, request).await?;
    let HostResponse::SessionStarted { session_id } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    if let Err(error) = task_store(state)?.bind_worker_session(worker_id, session_id) {
        let _ = request_host(state, HostRequest::Stop { session_id }).await;
        return Err(task_store_error(&error));
    }
    state.worker_errors.write().await.remove(&worker_id);
    state.control_room_notify.notify_waiters();
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(worker_view(
        profile,
        true,
        false,
        None,
        ProviderActivity::Unknown,
    ))
}

pub(super) async fn reconcile_worker_bindings(
    state: &AppState,
) -> Result<HashSet<WorkerSessionId>, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    reconcile_worker_bindings_unlocked(state).await
}

async fn reconcile_worker_bindings_unlocked(
    state: &AppState,
) -> Result<HashSet<WorkerSessionId>, ApiError> {
    let response = request_host(state, HostRequest::ListSessions).await?;
    let HostResponse::Sessions { sessions } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    let live = sessions
        .into_iter()
        .filter(|session| session.running)
        .map(|session| session.session_id)
        .collect::<HashSet<_>>();
    let released = task_store(state)?
        .release_missing_worker_sessions(&live)
        .map_err(|error| task_store_error(&error))?;
    if released > 0 {
        state.control_room_notify.notify_waiters();
    }
    Ok(live)
}
