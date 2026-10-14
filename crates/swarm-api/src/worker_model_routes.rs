//! The worker model's route (ADR 0116): the model this Hive's Claude repository
//! workers start with, and how many running ones started on another.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::WorkerModel;
use swarm_persistence::WorkerModelSetting;

use super::{ApiError, AppState, authorize, task_store, task_store_error, unix_timestamp};

/// The model in effect, the one this Hive's Apiary sets, if any (that one is
/// locked here and only its Keeper can change it, ADR 0113), and the running
/// workers that keep another model until they next start.
#[derive(Serialize)]
struct WorkerModelView {
    model: WorkerModel,
    set_by_apiary: Option<WorkerModel>,
    workers_short: u32,
}

impl From<WorkerModelSetting> for WorkerModelView {
    fn from(setting: WorkerModelSetting) -> Self {
        Self {
            model: setting.model,
            set_by_apiary: setting.set_by_apiary,
            workers_short: setting.workers_short,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetWorkerModelRequest {
    model: WorkerModel,
}

pub(super) async fn worker_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let setting = task_store(&state)?
        .worker_model()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerModelView::from(setting)),
    )
        .into_response())
}

pub(super) async fn set_worker_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetWorkerModelRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let setting = task_store(&state)?
        .set_worker_model(request.model, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerModelView::from(setting)),
    )
        .into_response())
}
