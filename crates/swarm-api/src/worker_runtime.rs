use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use axum::http::StatusCode;
use swarm_domain::{ProviderKind, WorkerId, WorkerProfile, WorkerSessionId};
use swarm_terminal::{
    ClaudeConversationStart, CodexConversationStart, HostRequest, HostResponse, TerminalSize,
};

use crate::{
    ApiError, AppState, WorkerViewFacts, task_store, task_store_error, terminal_host::request_host,
    worker_view,
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
    let is_scout = worker_is_scout(state, worker_id)?;
    if let Some(session_id) = profile.active_session_id
        && live.contains_key(&session_id)
    {
        let last_output_at = live.get(&session_id).copied().flatten();
        return Ok(worker_view(
            profile,
            WorkerViewFacts {
                running: true,
                system_role: is_scout.then_some("scout"),
                last_output_at,
                ..WorkerViewFacts::default()
            },
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

    let request = provider_start_request(state, worker_id, &profile, size, mcp_config)?;
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
    let is_scout = worker_is_scout(state, worker_id)?;
    Ok(worker_view(
        profile,
        WorkerViewFacts {
            running: true,
            system_role: is_scout.then_some("scout"),
            ..WorkerViewFacts::default()
        },
    ))
}

fn provider_start_request(
    state: &AppState,
    worker_id: WorkerId,
    profile: &WorkerProfile,
    size: TerminalSize,
    mcp_config: Option<PathBuf>,
) -> Result<HostRequest, ApiError> {
    let worker_workspace = PathBuf::from(&profile.workspace);
    let allow_outside_roots = !state
        .workspace_roots
        .iter()
        .any(|root| worker_workspace.starts_with(root));
    let request = match profile.provider {
        ProviderKind::ClaudeCode => {
            ensure_claude_resume_history(profile)?;
            HostRequest::StartClaude {
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
            }
        }
        ProviderKind::Codex => HostRequest::StartCodex {
            workspace: worker_workspace,
            size,
            conversation: match (
                profile.provider_conversation_id,
                profile.has_session_history,
            ) {
                (Some(session_id), true) => CodexConversationStart::Resume { session_id },
                (None, true) => CodexConversationStart::Continue,
                _ => CodexConversationStart::New,
            },
            allow_outside_roots,
        },
    };
    Ok(request)
}

fn ensure_claude_resume_history(profile: &WorkerProfile) -> Result<(), ApiError> {
    let Some(conversation_id) = profile
        .provider_conversation_id
        .filter(|_| profile.has_session_history)
    else {
        return Ok(());
    };
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| unavailable_conversation(&profile.name))?;
    let target =
        std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| home.join(".claude"), PathBuf::from);
    ensure_claude_resume_history_between(
        &profile.workspace,
        &conversation_id.to_string(),
        &home.join(".claude/projects"),
        &target.join("projects"),
        &home,
    )
    .map_err(|_| unavailable_conversation(&profile.name))
}

fn unavailable_conversation(worker_name: &str) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "provider_conversation_unavailable",
        format!("The saved Claude conversation for {worker_name} is not available on this machine"),
    )
}

fn ensure_claude_resume_history_between(
    workspace: &str,
    conversation_id: &str,
    source_root: &Path,
    target_root: &Path,
    home: &Path,
) -> Result<(), std::io::Error> {
    let workspace = if workspace == "~" {
        home.to_string_lossy().into_owned()
    } else if let Some(relative) = workspace.strip_prefix("~/") {
        home.join(relative).to_string_lossy().into_owned()
    } else {
        workspace.to_owned()
    };
    let encoded = workspace.replace(['/', '.'], "-");
    let destination_directory = target_root.join(&encoded);
    let destination = destination_directory.join(format!("{conversation_id}.jsonl"));
    if destination.is_file() {
        return Ok(());
    }
    let older_encoded = workspace.replace('/', "-");
    let source = [encoded.as_str(), older_encoded.as_str()]
        .into_iter()
        .map(|directory| {
            source_root
                .join(directory)
                .join(format!("{conversation_id}.jsonl"))
        })
        .find(|path| path.is_file())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "conversation missing"))?;
    std::fs::create_dir_all(&destination_directory)?;
    let temporary = destination.with_extension("jsonl.importing");
    std::fs::copy(source, &temporary)?;
    std::fs::rename(temporary, destination)
}

fn worker_is_scout(state: &AppState, worker_id: WorkerId) -> Result<bool, ApiError> {
    task_store(state)?
        .scout_worker_id()
        .map(|scout_id| scout_id == Some(worker_id))
        .map_err(|error| task_store_error(&error))
}

/// Live worker sessions and, where the terminal host reports it, the wall-clock
/// second each one last produced output.
pub(super) type LiveSessions = HashMap<WorkerSessionId, Option<i64>>;

pub(super) async fn reconcile_worker_bindings(state: &AppState) -> Result<LiveSessions, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    reconcile_worker_bindings_unlocked(state).await
}

async fn reconcile_worker_bindings_unlocked(state: &AppState) -> Result<LiveSessions, ApiError> {
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
        .map(|session| (session.session_id, session.last_output_at))
        .collect::<LiveSessions>();
    let live_ids = live.keys().copied().collect::<HashSet<_>>();
    let released = task_store(state)?
        .release_missing_worker_sessions(&live_ids)
        .map_err(|error| task_store_error(&error))?;
    if released > 0 {
        // Releasing a session detaches a worker from its profile: the roster
        // shows it sleeping while its terminal keeps running under a generated
        // name. That is a large, visible change to make silently, and it went
        // unexplained for hours once because nothing recorded it. An empty
        // report from the host releases every session at once, so the count the
        // host gave is recorded beside the count released.
        tracing::warn!(
            released,
            host_reported_running = live_ids.len(),
            "worker sessions were released because the terminal host no longer reports them"
        );
        state.control_room_notify.notify_waiters();
    }
    Ok(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_wake_stages_an_existing_legacy_claude_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let source_root = home.join(".claude/projects");
        let target_root = home.join("isolated/projects");
        let conversation_id = "8e9ed267-7ed8-4b64-94ef-dde3ab17f21a";
        let source = source_root.join("-home-projects-petal");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join(format!("{conversation_id}.jsonl")), b"history").unwrap();

        ensure_claude_resume_history_between(
            "~/projects/petal",
            conversation_id,
            &source_root,
            &target_root,
            Path::new("/home"),
        )
        .unwrap();

        assert_eq!(
            std::fs::read(
                target_root
                    .join("-home-projects-petal")
                    .join(format!("{conversation_id}.jsonl"))
            )
            .unwrap(),
            b"history"
        );
    }

    #[test]
    fn first_wake_fails_closed_when_the_saved_conversation_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let error = ensure_claude_resume_history_between(
            "/home/projects/petal",
            "8e9ed267-7ed8-4b64-94ef-dde3ab17f21a",
            &directory.path().join("source"),
            &directory.path().join("target"),
            Path::new("/home"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}
