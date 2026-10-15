//! Terminal history retention's route and its reach into the terminal host
//! (ADR 0118): how long this Hive keeps terminal history, and whether its host
//! is keeping exactly that.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_application::ApiaryService;
use swarm_domain::{HistoryRetentionDays, HostRetention};
use swarm_persistence::TerminalHistorySetting;
use swarm_terminal::{
    history_retention_applied_path, history_retention_request_path, read_history_retention,
    write_history_retention,
};

use super::{ApiError, AppState, authorize, task_store, task_store_error, unix_timestamp};

impl AppState {
    /// Brings the terminal host in line with this Hive's retention: asks for
    /// the days in effect when the request on disk differs, and records what
    /// the host says it is keeping. A member then re-applies its Apiary's
    /// requirements, so required days count as in effect only once the host
    /// keeps them.
    ///
    /// Never fails the caller. A Hive whose retention cannot be written keeps
    /// what its host already has, and its card says the setting is not in
    /// effect, which is the truth.
    pub fn sync_terminal_history(&self) -> Option<TerminalHistorySetting> {
        let dir = self.terminal_history_dir.as_ref()?;
        let store = self.task_store.as_ref()?;
        let wanted = store
            .terminal_history()
            .inspect_err(|error| tracing::warn!(%error, "terminal history retention unreadable"))
            .ok()?
            .days;
        let request = history_retention_request_path(dir);
        if read_history_retention(&request) != Some(wanted.seconds())
            && let Err(error) = write_history_retention(&request, wanted.seconds())
        {
            tracing::warn!(%error, "could not ask the terminal host for this Hive's retention");
        }
        let now = unix_timestamp();
        let applied = read_history_retention(&history_retention_applied_path(dir));
        let setting = store
            .record_terminal_history_host(applied, now)
            .inspect_err(|error| tracing::warn!(%error, "could not record the host's retention"))
            .ok()?;
        if let Err(error) = ApiaryService::new(store.clone()).enforce_required_policy(now) {
            tracing::warn!(%error, "required settings could not be re-applied");
        }
        Some(setting)
    }
}

/// What the terminal host is doing about it, for the card.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum HostView {
    /// It keeps exactly these days.
    InEffect,
    /// It keeps something else and will pick these up within a minute.
    Applying,
    /// It has never said: it is older than the setting, and takes it with the
    /// worker engine update.
    AwaitingEngineUpdate,
    /// Not looked at yet.
    Unknown,
}

#[derive(Serialize)]
struct TerminalHistoryView {
    days: HistoryRetentionDays,
    set_by_apiary: Option<HistoryRetentionDays>,
    host: HostView,
    /// What the host says it keeps, in seconds, when it has said.
    host_max_age_seconds: Option<u64>,
}

impl From<TerminalHistorySetting> for TerminalHistoryView {
    fn from(setting: TerminalHistorySetting) -> Self {
        let (host, host_max_age_seconds) = match setting.host {
            HostRetention::Using { max_age_seconds } => (
                if setting.in_effect {
                    HostView::InEffect
                } else {
                    HostView::Applying
                },
                Some(max_age_seconds),
            ),
            HostRetention::Silent => (HostView::AwaitingEngineUpdate, None),
            HostRetention::Unknown => (HostView::Unknown, None),
        };
        Self {
            days: setting.days,
            set_by_apiary: setting.set_by_apiary,
            host,
            host_max_age_seconds,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetTerminalHistoryRequest {
    days: HistoryRetentionDays,
}

fn view(state: &AppState) -> Result<Response, ApiError> {
    // Read through the host each time, so the card never shows "in effect"
    // from a moment the host has since moved on from.
    let setting = match state.sync_terminal_history() {
        Some(setting) => setting,
        None => task_store(state)?
            .terminal_history()
            .map_err(|error| task_store_error(&error))?,
    };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(TerminalHistoryView::from(setting)),
    )
        .into_response())
}

pub(super) async fn terminal_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    view(&state)
}

pub(super) async fn set_terminal_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetTerminalHistoryRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_store(&state)?
        .set_terminal_history_days(request.days, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    view(&state)
}
