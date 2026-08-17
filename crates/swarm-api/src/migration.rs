use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_persistence::{LegacyMigrationBundle, LegacyMigrationCommit};

use super::{ApiError, AppState, authorize, task_store, task_store_error};

pub const MAX_MIGRATION_BUNDLE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(super) struct CommitLegacyMigrationRequest {
    bundle: LegacyMigrationBundle,
    commit: LegacyMigrationCommit,
}

#[derive(Debug, Deserialize)]
pub(super) struct RollbackLegacyMigrationRequest {
    batch_id: String,
    bundle_digest: String,
}

pub(super) async fn preview_legacy_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(bundle): Json<LegacyMigrationBundle>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let preview = task_store(&state)?
        .preview_legacy_task_migration(&bundle)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(preview).into_response())
}

pub(super) async fn list_active_legacy_task_migrations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let receipts = task_store(&state)?
        .list_active_legacy_migration_receipts()
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(receipts).into_response())
}

pub(super) async fn commit_legacy_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CommitLegacyMigrationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let receipt = task_store(&state)?
        .commit_legacy_task_migration(&request.bundle, &request.commit)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(receipt)).into_response())
}

pub(super) async fn rollback_legacy_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RollbackLegacyMigrationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let rollback = task_store(&state)?
        .rollback_legacy_task_migration(&request.batch_id, &request.bundle_digest)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(rollback).into_response())
}
