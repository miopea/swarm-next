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
use swarm_domain::{ControlRoomEventKind, WorkerProfile, WorkerSessionId};
use swarm_terminal::{HostRequest, HostResponse, ProviderActivity, classify_provider_activity};

use crate::{ApiError, AppState, authorize, request_host};

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct ProviderCapabilitiesView {
    claude_code: bool,
    codex: bool,
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
                }) => Some((session_id, classify_provider_activity(provider, &snapshot))),
                _ => None,
            }
        })
        .buffer_unordered(8)
        .filter_map(async move |observation| observation)
        .collect()
        .await
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
        Ok(HostResponse::ProviderCapabilities { claude_code, codex }) => {
            ProviderCapabilitiesView { claude_code, codex }
        }
        _ => ProviderCapabilitiesView {
            claude_code: true,
            codex: false,
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(capabilities)).into_response())
}
