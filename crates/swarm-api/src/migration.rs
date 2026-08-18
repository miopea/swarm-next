use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_persistence::{
    LegacyMigrationBundle, LegacyMigrationCommit, LegacyWorkerMigrationCommit,
};

use super::{ApiError, AppState, authorize, task_store, task_store_error};

pub const MAX_MIGRATION_BUNDLE_BYTES: usize = 16 * 1024 * 1024;

pub(super) async fn discover_local_legacy_migration(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let path = state.legacy_database_path.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "legacy_hive_not_found",
            "No local Legacy Hive is configured on this machine",
        )
    })?;
    if !path.is_file() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "legacy_hive_not_found",
            "No local Legacy Hive was found on this machine",
        ));
    }
    let path = path.clone();
    let bundle =
        tokio::task::spawn_blocking(move || swarm_persistence::read_legacy_migration_bundle(path))
            .await
            .map_err(|_| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "legacy_hive_read_failed",
                    "Swarm could not finish reading the local Legacy Hive",
                )
            })?
            .map_err(|_| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "legacy_hive_invalid",
                    "The local Legacy Hive could not be safely prepared for migration",
                )
            })?;
    let encoded = serde_json::to_vec(&bundle).map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "legacy_hive_read_failed",
            "Swarm could not prepare the local Legacy Hive preview",
        )
    })?;
    if encoded.len() > MAX_MIGRATION_BUNDLE_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "legacy_hive_too_large",
            "The local Legacy Hive contains too much open work for one migration preview",
        ));
    }
    Ok(Json(bundle).into_response())
}

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

#[derive(Debug, Deserialize)]
pub(super) struct CommitLegacyWorkerMigrationRequest {
    bundle: LegacyMigrationBundle,
    commit: LegacyWorkerMigrationCommit,
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

pub(super) async fn preview_legacy_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(bundle): Json<LegacyMigrationBundle>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let preview = task_store(&state)?
        .preview_legacy_worker_migration(&bundle)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(preview).into_response())
}

pub(super) async fn list_active_legacy_worker_migrations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let receipts = task_store(&state)?
        .list_active_legacy_worker_migration_receipts()
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(receipts).into_response())
}

pub(super) async fn commit_legacy_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CommitLegacyWorkerMigrationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let receipt = task_store(&state)?
        .commit_legacy_worker_migration(&request.bundle, &request.commit)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(receipt)).into_response())
}

pub(super) async fn rollback_legacy_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RollbackLegacyMigrationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let rollback = task_store(&state)?
        .rollback_legacy_worker_migration(&request.batch_id, &request.bundle_digest)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(rollback).into_response())
}
