use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

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
    if request.commit.resume_legacy_conversations {
        stage_legacy_conversations(&request.bundle, &request.commit).map_err(|error| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "legacy_conversation_unavailable",
                error,
            )
        })?;
    }
    let receipt = task_store(&state)?
        .commit_legacy_worker_migration(&request.bundle, &request.commit)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(receipt)).into_response())
}

fn stage_legacy_conversations(
    bundle: &LegacyMigrationBundle,
    commit: &LegacyWorkerMigrationCommit,
) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "Swarm could not locate this operator's home folder".to_owned())?;
    let target =
        std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| home.join(".claude"), PathBuf::from);
    stage_legacy_conversations_between(
        bundle,
        commit,
        &home.join(".claude").join("projects"),
        &target.join("projects"),
        &home,
    )
}

fn stage_legacy_conversations_between(
    bundle: &LegacyMigrationBundle,
    commit: &LegacyWorkerMigrationCommit,
    source_root: &Path,
    target_root: &Path,
    home: &Path,
) -> Result<(), String> {
    let selected = commit.selected_source_ids.iter().collect::<HashSet<_>>();
    for worker in bundle.workers.iter().filter(|worker| {
        selected.contains(&worker.source_id)
            && matches!(
                worker.provider.trim().to_ascii_lowercase().as_str(),
                "" | "claude" | "claude_code"
            )
    }) {
        let Some(conversation_id) = worker.provider_conversation_id.as_deref() else {
            continue;
        };
        let workspace = expand_home(&worker.workspace, home);
        let encoded = workspace.replace(['/', '.'], "-");
        let older_encoded = workspace.replace('/', "-");
        let source = [encoded.as_str(), older_encoded.as_str()]
            .into_iter()
            .map(|directory| {
                source_root
                    .join(directory)
                    .join(format!("{conversation_id}.jsonl"))
            })
            .find(|path| path.is_file())
            .ok_or_else(|| {
                format!(
                    "The exact Claude conversation for {} is no longer available",
                    worker.name
                )
            })?;
        let destination_directory = target_root.join(encoded);
        std::fs::create_dir_all(&destination_directory).map_err(|_| {
            format!(
                "Swarm could not prepare the Claude history folder for {}",
                worker.name
            )
        })?;
        let destination = destination_directory.join(format!("{conversation_id}.jsonl"));
        if destination.is_file() {
            continue;
        }
        let temporary = destination.with_extension("jsonl.importing");
        std::fs::copy(&source, &temporary).map_err(|_| {
            format!(
                "Swarm could not preserve the Claude conversation for {}",
                worker.name
            )
        })?;
        std::fs::rename(&temporary, &destination).map_err(|_| {
            format!(
                "Swarm could not activate the Claude conversation for {}",
                worker.name
            )
        })?;
    }
    Ok(())
}

fn expand_home(workspace: &str, home: &Path) -> String {
    let workspace = workspace.trim();
    if workspace == "~" {
        return home.to_string_lossy().into_owned();
    }
    workspace.strip_prefix("~/").map_or_else(
        || workspace.to_owned(),
        |relative| home.join(relative).to_string_lossy().into_owned(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_persistence::{LegacyMigrationSource, LegacyWorkerRecord};

    fn worker_bundle(conversation_id: &str) -> LegacyMigrationBundle {
        LegacyMigrationBundle {
            format: "swarm-next-migration".into(),
            version: 1,
            source: LegacyMigrationSource {
                installation_id: "legacy-local".into(),
                schema_version: Some(1),
                exported_at: 1,
                snapshot_digest: "digest".into(),
            },
            tasks: vec![],
            workers: vec![LegacyWorkerRecord {
                source_id: "petal".into(),
                name: "Petal".into(),
                workspace: "~/projects/petal".into(),
                description: String::new(),
                provider: "claude".into(),
                position: 0,
                has_identity_file: false,
                isolation: String::new(),
                provider_conversation_id: Some(conversation_id.into()),
            }],
        }
    }

    #[test]
    fn selected_claude_history_is_staged_for_the_isolated_profile() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let source_root = home.join(".claude/projects");
        let target_root = home.join(".swarm-next-claude/projects");
        let conversation_id = "8e9ed267-7ed8-4b64-94ef-dde3ab17f21a";
        let project = source_root.join("-home-projects-petal");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(format!("{conversation_id}.jsonl")), b"history").unwrap();
        let commit = LegacyWorkerMigrationCommit {
            bundle_digest: "digest".into(),
            selected_source_ids: vec!["petal".into()],
            resume_legacy_conversations: true,
            replace_existing_conversations: false,
        };

        stage_legacy_conversations_between(
            &worker_bundle(conversation_id),
            &commit,
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
    fn missing_selected_history_prevents_a_false_resume_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let commit = LegacyWorkerMigrationCommit {
            bundle_digest: "digest".into(),
            selected_source_ids: vec!["petal".into()],
            resume_legacy_conversations: true,
            replace_existing_conversations: false,
        };
        let error = stage_legacy_conversations_between(
            &worker_bundle("8e9ed267-7ed8-4b64-94ef-dde3ab17f21a"),
            &commit,
            &directory.path().join("source"),
            &directory.path().join("target"),
            Path::new("/home"),
        )
        .unwrap_err();
        assert!(error.contains("no longer available"));
    }
}
