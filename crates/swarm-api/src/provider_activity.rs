use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream};
use serde::Serialize;
use swarm_domain::ProviderKind;
use swarm_domain::{ControlRoomEventKind, WorkerProfile, WorkerSessionId};
use swarm_terminal::{
    HostRequest, HostResponse, ProviderActivity, TerminalSnapshot, classify_provider_activity,
};

use crate::{ApiError, AppState, authorize, terminal_host::request_host};

#[derive(Clone, Debug, Serialize)]
pub(super) struct ProviderCapabilitiesView {
    claude_code: bool,
    codex: bool,
    /// The release each provider resolves to now, and the workers still running
    /// something older.
    ///
    /// Claude and Codex update themselves and the running process keeps
    /// executing what it started with, so an update installed while workers are
    /// up is not running anywhere until each one restarts.
    #[serde(default)]
    superseded: Vec<SupersededProviderView>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct SupersededProviderView {
    provider: &'static str,
    version: Option<String>,
    installed_at: Option<i64>,
    worker_ids: Vec<String>,
}

/// The workers still running a provider release that disk has moved past.
///
/// Reports nothing rather than guessing when the roster cannot be read: a
/// restart prompt that is not needed teaches the operator to ignore the one
/// that is.
fn superseded_providers(
    state: &AppState,
    claude_release: Option<&swarm_terminal::ProviderRelease>,
    codex_release: Option<&swarm_terminal::ProviderRelease>,
) -> Vec<SupersededProviderView> {
    let Ok(sessions) = crate::task_store(state).and_then(|store| {
        store
            .active_worker_sessions()
            .map_err(|error| crate::task_store_error(&error))
    }) else {
        return Vec::new();
    };
    [
        (
            "claude_code",
            swarm_domain::ProviderKind::ClaudeCode,
            claude_release,
        ),
        ("codex", swarm_domain::ProviderKind::Codex, codex_release),
    ]
    .into_iter()
    .filter_map(|(name, kind, release)| {
        let worker_ids = sessions
            .iter()
            .filter(|session| {
                session.provider == kind
                    && swarm_terminal::provider_release_superseded(release, session.started_at)
            })
            .map(|session| session.worker_id.to_string())
            .collect::<Vec<_>>();
        (!worker_ids.is_empty()).then(|| SupersededProviderView {
            provider: name,
            version: release.and_then(|release| release.version.clone()),
            installed_at: release.and_then(|release| release.installed_at),
            worker_ids,
        })
    })
    .collect()
}

async fn observe(
    state: &AppState,
    profiles: &[WorkerProfile],
    live: &HashSet<WorkerSessionId>,
) -> HashMap<WorkerSessionId, ProviderActivity> {
    let observations = profiles
        .iter()
        .filter_map(|profile| {
            let session_id = profile.active_session_id?;
            live.contains(&session_id)
                .then_some((session_id, profile.provider))
        })
        .collect::<Vec<_>>();
    stream::iter(observations)
        .map(|(session_id, provider)| async move {
            observe_session(state, session_id, provider)
                .await
                .map(|activity| (session_id, activity))
        })
        .buffer_unordered(8)
        .filter_map(async move |observation| observation)
        .collect()
        .await
}

/// Reads the exact current host-owned snapshot for one provider session.
///
/// Coordination delivery uses this immediately before writing so an open
/// provider question is an input-authority boundary rather than a visual hint.
pub(super) async fn observe_session(
    state: &AppState,
    session_id: WorkerSessionId,
    provider: ProviderKind,
) -> Option<ProviderActivity> {
    match request_host(
        state,
        HostRequest::Read {
            session_id,
            after_sequence: None,
        },
    )
    .await
    {
        Ok(HostResponse::Output {
            resume: swarm_terminal::Resume::Snapshot { snapshot },
            running: true,
            ..
        }) => Some(classify_observed_activity(provider, &snapshot)),
        _ => None,
    }
}

/// Adds application-level provider UI recognition without changing the
/// independently deployed terminal engine. Claude can leave a slash-command
/// palette open below its input row; enough suggestions can push the prompt
/// outside the terminal crate's deliberately small recent-line window even
/// though the provider is visibly waiting for input.
pub(super) fn classify_observed_activity(
    provider: ProviderKind,
    snapshot: &TerminalSnapshot,
) -> ProviderActivity {
    let activity = classify_provider_activity(provider, snapshot);
    if activity != ProviderActivity::Unknown {
        return activity;
    }
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    if input_palette_is_resting(provider, &parser.screen().contents()) {
        ProviderActivity::Resting
    } else {
        activity
    }
}

/// Returns true when the provider is idle but its current prompt already owns
/// unsubmitted text. A resting footer is not permission to append another
/// coordination message: doing so merges independent tasks and decisions into
/// one prompt and makes a later Enter ambiguous.
pub(super) fn has_open_provider_input(provider: ProviderKind, snapshot: &TerminalSnapshot) -> bool {
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    parser
        .screen()
        .contents()
        .lines()
        .rev()
        .map(str::trim)
        .take(12)
        .any(|line| match provider {
            ProviderKind::ClaudeCode => line
                .strip_prefix('❯')
                .is_some_and(|tail| !tail.trim().is_empty()),
            ProviderKind::Codex => line
                .strip_prefix('›')
                .is_some_and(|tail| !tail.trim().is_empty()),
        })
}

fn input_palette_is_resting(provider: ProviderKind, visible: &str) -> bool {
    let lines = visible.lines().map(str::trim).collect::<Vec<_>>();
    let Some(prompt_index) = lines.iter().rposition(|line| match provider {
        ProviderKind::ClaudeCode => line == &"❯ /" || line.starts_with("❯ /"),
        ProviderKind::Codex => line == &"› /" || line.starts_with("› /"),
    }) else {
        return false;
    };
    lines[prompt_index + 1..]
        .iter()
        .filter(|line| line.starts_with('/') && line.len() > 1)
        .take(2)
        .count()
        >= 2
}

pub(super) async fn refresh(
    state: &AppState,
    profiles: &[WorkerProfile],
    live: &HashSet<WorkerSessionId>,
) -> HashMap<WorkerSessionId, ProviderActivity> {
    let observed = observe(state, profiles, live).await;
    let changed = {
        let mut previous = state.provider_activity.write().await;
        if *previous == observed {
            false
        } else {
            previous.clone_from(&observed);
            true
        }
    };
    if changed {
        if let Some(store) = &state.task_store
            && let Err(error) =
                store.record_control_room_event(ControlRoomEventKind::RuntimeChanged)
        {
            tracing::warn!(%error, "provider activity change could not publish its runtime event");
        }
        state.control_room_notify.notify_waiters();
    }
    observed
}

pub(super) async fn capabilities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let capabilities = match request_host(&state, HostRequest::ProviderCapabilities).await {
        Ok(HostResponse::ProviderCapabilities {
            claude_code,
            codex,
            claude_release,
            codex_release,
        }) => ProviderCapabilitiesView {
            claude_code,
            codex,
            superseded: superseded_providers(
                &state,
                claude_release.as_ref(),
                codex_release.as_ref(),
            ),
        },
        _ => ProviderCapabilitiesView {
            claude_code: true,
            codex: false,
            superseded: Vec::new(),
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(capabilities)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_terminal::{CanonicalTerminalState, JournalLimits, TerminalSize};

    fn snapshot(text: &str) -> TerminalSnapshot {
        let mut state = CanonicalTerminalState::new(
            JournalLimits::new(64, 64 * 1024),
            TerminalSize::new(24, 100),
        );
        state.push(text.as_bytes().to_vec());
        state.snapshot()
    }

    #[test]
    fn claude_slash_palette_remains_resting_when_suggestions_hide_the_prompt() {
        let suggestions = (0..18)
            .map(|index| format!("/command-{index}  Description {index}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let snapshot = snapshot(&format!("✻ Worked for 18s\r\n\r\n❯ /\r\n{suggestions}"));

        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Unknown,
            "the base classifier stays conservative once the prompt leaves its recent window"
        );
        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Resting
        );
    }

    #[test]
    fn typed_provider_input_is_not_an_idle_delivery_target() {
        assert!(has_open_provider_input(
            ProviderKind::ClaudeCode,
            &snapshot("❯ [Swarm decision 01ab23cd resolved]\r\nauto mode on"),
        ));
        assert!(!has_open_provider_input(
            ProviderKind::ClaudeCode,
            &snapshot("✻ Finished\r\n❯ \r\nauto mode on"),
        ));
    }

    #[test]
    fn historical_commands_do_not_turn_unknown_output_into_resting() {
        let output = (0..12)
            .map(|index| format!("provider output changed {index}"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let snapshot = snapshot(&format!("❯ /status\r\n/one historical path\r\n{output}"));

        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Unknown
        );
        assert_eq!(
            classify_observed_activity(ProviderKind::ClaudeCode, &snapshot),
            ProviderActivity::Unknown
        );
    }
}
