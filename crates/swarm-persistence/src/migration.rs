use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarm_domain::{
    ProviderKind, TaskActivityActorKind, TaskId, TaskPriority, TaskState, WorkerId, WorkerRole,
};
use uuid::Uuid;

use super::{
    ControlRoomEventKind, MAX_TASK_DESCRIPTION_BYTES, MAX_TASK_TITLE_BYTES, TaskStore,
    TaskStoreError, insert_control_room_event,
};

pub const LEGACY_MIGRATION_FORMAT: &str = "swarm-next-migration";
pub const LEGACY_MIGRATION_VERSION: u16 = 1;
pub const MAX_MIGRATION_TASKS: usize = 10_000;
pub const MAX_MIGRATION_WORKERS: usize = 1_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyMigrationBundle {
    pub format: String,
    pub version: u16,
    pub source: LegacyMigrationSource,
    #[serde(default)]
    pub tasks: Vec<LegacyTaskRecord>,
    #[serde(default)]
    pub workers: Vec<LegacyWorkerRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyWorkerRecord {
    pub source_id: String,
    pub name: String,
    pub workspace: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provider: String,
    pub position: i64,
    #[serde(default)]
    pub has_identity_file: bool,
    #[serde(default)]
    pub isolation: String,
    /// Exact provider-owned conversation discovered from the local provider
    /// transcript store. The transcript itself is never included.
    #[serde(default)]
    pub provider_conversation_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyMigrationSource {
    pub installation_id: String,
    pub schema_version: Option<i64>,
    pub exported_at: i64,
    pub snapshot_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyTaskRecord {
    pub source_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: String,
    #[serde(default)]
    pub priority: String,
    pub assigned_worker: Option<String>,
    pub jira_key: Option<String>,
    pub block_reason: Option<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub attachment_count: usize,
    pub source_email_id: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyImportDisposition {
    Ready,
    Transformed,
    Duplicate,
    SkippedJira,
    SkippedClosed,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyWorkerImportDisposition {
    Ready,
    Transformed,
    Duplicate,
    ManagedByNext,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyWorkerPreview {
    pub source_id: String,
    pub name: String,
    pub workspace: String,
    pub provider: ProviderKind,
    pub disposition: LegacyWorkerImportDisposition,
    pub selectable: bool,
    pub conversation_available: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyWorkerMigrationPreview {
    pub bundle_digest: String,
    pub source_installation_id: String,
    pub records: Vec<LegacyWorkerPreview>,
    pub selectable: usize,
    pub skipped: usize,
    pub invalid: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyWorkerMigrationCommit {
    pub bundle_digest: String,
    pub selected_source_ids: Vec<String>,
    #[serde(default)]
    pub resume_legacy_conversations: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyWorkerMigrationReceipt {
    pub batch_id: String,
    pub bundle_digest: String,
    pub source_installation_id: String,
    pub imported_worker_ids: Vec<WorkerId>,
    pub imported_source_ids: Vec<String>,
    pub resumed_source_ids: Vec<String>,
    pub imported_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyWorkerMigrationRollback {
    pub batch_id: String,
    pub removed_workers: usize,
    pub rolled_back_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyTaskPreview {
    pub source_id: String,
    pub title: String,
    pub source_status: String,
    pub target_state: Option<TaskState>,
    pub priority: TaskPriority,
    pub matched_worker_id: Option<WorkerId>,
    pub matched_worker_name: Option<String>,
    pub disposition: LegacyImportDisposition,
    pub selectable: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyMigrationPreview {
    pub bundle_digest: String,
    pub source_installation_id: String,
    pub records: Vec<LegacyTaskPreview>,
    pub selectable: usize,
    pub skipped: usize,
    pub invalid: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyMigrationCommit {
    pub bundle_digest: String,
    pub selected_source_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyMigrationReceipt {
    pub batch_id: String,
    pub bundle_digest: String,
    pub source_installation_id: String,
    pub source_snapshot_digest: String,
    pub imported_task_ids: Vec<TaskId>,
    pub imported_source_ids: Vec<String>,
    pub imported_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LegacyMigrationRollback {
    pub batch_id: String,
    pub removed_tasks: usize,
    pub rolled_back_at: i64,
}

#[derive(Clone)]
struct NormalizedTask {
    source_id: String,
    title: String,
    description: String,
    source_status: String,
    state: TaskState,
    priority: TaskPriority,
    workspace: String,
    worker_id: Option<WorkerId>,
    worker_name: Option<String>,
    disposition: LegacyImportDisposition,
    selectable: bool,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct NormalizedWorker {
    source_id: String,
    name: String,
    workspace: String,
    description: String,
    provider: ProviderKind,
    provider_conversation_id: Option<swarm_domain::ProviderConversationId>,
    disposition: LegacyWorkerImportDisposition,
    selectable: bool,
    warnings: Vec<String>,
}

impl TaskStore {
    /// Returns reversible worker-import receipts that have not been rolled back.
    ///
    /// # Errors
    ///
    /// Returns an error when migration records cannot be read or contain an
    /// invalid worker identifier.
    pub fn list_active_legacy_worker_migration_receipts(
        &self,
    ) -> Result<Vec<LegacyWorkerMigrationReceipt>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT batch.id, batch.source_digest, batch.source_installation_id,
                    batch.imported_at, link.worker_id, link.source_worker_id,
                    link.resumed_conversation
             FROM migration_worker_batches batch
             JOIN migration_worker_links link ON link.batch_id = batch.id
             WHERE batch.rolled_back_at IS NULL
             ORDER BY batch.imported_at DESC, batch.id, link.source_worker_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, bool>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut receipts = Vec::<LegacyWorkerMigrationReceipt>::new();
        for (batch_id, digest, installation_id, imported_at, worker_id, source_id, resumed) in rows
        {
            if receipts
                .last()
                .is_none_or(|receipt| receipt.batch_id != batch_id)
            {
                receipts.push(LegacyWorkerMigrationReceipt {
                    batch_id,
                    bundle_digest: digest,
                    source_installation_id: installation_id,
                    imported_worker_ids: Vec::new(),
                    imported_source_ids: Vec::new(),
                    resumed_source_ids: Vec::new(),
                    imported_at,
                });
            }
            let Some(receipt) = receipts.last_mut() else {
                return Err(TaskStoreError::InvalidMigrationBundle);
            };
            receipt.imported_worker_ids.push(
                worker_id
                    .parse()
                    .map_err(|_| TaskStoreError::InvalidMigrationBundle)?,
            );
            if resumed {
                receipt.resumed_source_ids.push(source_id.clone());
            }
            receipt.imported_source_ids.push(source_id);
        }
        Ok(receipts)
    }

    /// Produces a read-only roster plan. Provider processes, conversations,
    /// identity files, approval rules, and group membership are never imported.
    ///
    /// # Errors
    ///
    /// Returns an error when the package is invalid or existing worker state
    /// cannot be read.
    pub fn preview_legacy_worker_migration(
        &self,
        bundle: &LegacyMigrationBundle,
    ) -> Result<LegacyWorkerMigrationPreview, TaskStoreError> {
        let digest = validate_bundle(bundle)?;
        let records = self
            .normalize_legacy_workers(bundle)?
            .into_iter()
            .map(|worker| LegacyWorkerPreview {
                source_id: worker.source_id,
                name: worker.name,
                workspace: worker.workspace,
                provider: worker.provider,
                disposition: worker.disposition,
                selectable: worker.selectable,
                conversation_available: worker.provider_conversation_id.is_some(),
                warnings: worker.warnings,
            })
            .collect::<Vec<_>>();
        Ok(LegacyWorkerMigrationPreview {
            bundle_digest: digest,
            source_installation_id: bundle.source.installation_id.clone(),
            selectable: records.iter().filter(|record| record.selectable).count(),
            skipped: records
                .iter()
                .filter(|record| {
                    matches!(
                        record.disposition,
                        LegacyWorkerImportDisposition::Duplicate
                            | LegacyWorkerImportDisposition::ManagedByNext
                    )
                })
                .count(),
            invalid: records
                .iter()
                .filter(|record| record.disposition == LegacyWorkerImportDisposition::Invalid)
                .count(),
            records,
        })
    }

    /// Imports an explicit worker selection as sleeping durable profiles.
    /// No provider process is started. Exact provider conversations are attached
    /// only when the operator explicitly selected that migration option.
    ///
    /// # Errors
    ///
    /// Returns an error when the package or selection changed, a duplicate is
    /// detected at commit time, or the profiles cannot be stored atomically.
    pub fn commit_legacy_worker_migration(
        &self,
        bundle: &LegacyMigrationBundle,
        commit: &LegacyWorkerMigrationCommit,
    ) -> Result<LegacyWorkerMigrationReceipt, TaskStoreError> {
        let digest = validate_bundle(bundle)?;
        if digest != commit.bundle_digest {
            return Err(TaskStoreError::MigrationBundleChanged);
        }
        let selected = commit
            .selected_source_ids
            .iter()
            .map(|id| id.trim().to_owned())
            .collect::<HashSet<_>>();
        if selected.is_empty() || selected.len() != commit.selected_source_ids.len() {
            return Err(TaskStoreError::InvalidMigrationSelection);
        }
        let workers = self
            .normalize_legacy_workers(bundle)?
            .into_iter()
            .filter(|worker| selected.contains(&worker.source_id))
            .collect::<Vec<_>>();
        if workers.len() != selected.len() || workers.iter().any(|worker| !worker.selectable) {
            return Err(TaskStoreError::InvalidMigrationSelection);
        }

        let identity = self.local_hive_identity()?;
        let batch_id = Uuid::now_v7().to_string();
        let imported_at = unix_timestamp();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO migration_worker_batches
             (id, source_installation_id, source_digest, imported_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![batch_id, bundle.source.installation_id, digest, imported_at],
        )?;
        let first_position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM worker_profiles
             WHERE hive_id = ?1 AND archived_at IS NULL",
            [identity.hive.id.to_string()],
            |row| row.get(0),
        )?;
        let mut imported_worker_ids = Vec::with_capacity(workers.len());
        let mut imported_source_ids = Vec::with_capacity(workers.len());
        let mut resumed_source_ids = Vec::with_capacity(workers.len());
        for (offset, worker) in workers.into_iter().enumerate() {
            let position = first_position
                + i64::try_from(offset).map_err(|_| TaskStoreError::InvalidMigrationSelection)?;
            let duplicate_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM worker_profiles
                 WHERE hive_id = ?1 AND archived_at IS NULL
                   AND (lower(trim(name)) = lower(trim(?2)) OR workspace = ?3))",
                params![identity.hive.id.to_string(), worker.name, worker.workspace],
                |row| row.get(0),
            )?;
            if duplicate_exists {
                return Err(TaskStoreError::MigrationBundleChanged);
            }
            let worker_id = WorkerId::new();
            let provider_conversation_id = commit
                .resume_legacy_conversations
                .then_some(worker.provider_conversation_id)
                .flatten();
            transaction.execute(
                "INSERT INTO worker_profiles
                 (id, hive_id, name, description, role, provider, workspace,
                  autostart, position, created_at, updated_at,
                  provider_conversation_id, provider_conversation_resume)
                 VALUES (?1, ?2, ?3, ?4, 'worker', ?5, ?6, 0, ?7, ?8, ?8, ?9, ?10)",
                params![
                    worker_id.to_string(),
                    identity.hive.id.to_string(),
                    worker.name,
                    worker.description,
                    worker.provider.to_string(),
                    worker.workspace,
                    position,
                    imported_at,
                    provider_conversation_id.map(|value| value.to_string()),
                    i64::from(provider_conversation_id.is_some()),
                ],
            )?;
            let digest = worker_profile_digest(
                &worker.name,
                &worker.description,
                worker.provider,
                &worker.workspace,
                position,
                imported_at,
            );
            transaction.execute(
                "INSERT INTO migration_worker_links
                 (batch_id, source_worker_id, worker_id, imported_profile_digest,
                  resumed_conversation)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    batch_id,
                    worker.source_id,
                    worker_id.to_string(),
                    digest,
                    i64::from(provider_conversation_id.is_some()),
                ],
            )?;
            imported_worker_ids.push(worker_id);
            if provider_conversation_id.is_some() {
                resumed_source_ids.push(worker.source_id.clone());
            }
            imported_source_ids.push(worker.source_id);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(LegacyWorkerMigrationReceipt {
            batch_id,
            bundle_digest: digest,
            source_installation_id: bundle.source.installation_id.clone(),
            imported_worker_ids,
            imported_source_ids,
            resumed_source_ids,
            imported_at,
        })
    }

    /// Removes an untouched worker-import batch. Any profile edit, session, or
    /// owned task makes rollback fail closed.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt is missing or changed, an imported
    /// profile has been used or edited, or the rollback cannot be committed.
    #[allow(clippy::too_many_lines)]
    pub fn rollback_legacy_worker_migration(
        &self,
        batch_id: &str,
        bundle_digest: &str,
    ) -> Result<LegacyWorkerMigrationRollback, TaskStoreError> {
        let batch_id = batch_id.trim();
        let bundle_digest = bundle_digest.trim();
        if batch_id.is_empty() || bundle_digest.is_empty() {
            return Err(TaskStoreError::MigrationBatchNotFound);
        }
        let rolled_back_at = unix_timestamp();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let stored_digest = transaction
            .query_row(
                "SELECT source_digest FROM migration_worker_batches
                 WHERE id = ?1 AND rolled_back_at IS NULL",
                [batch_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::MigrationBatchNotFound)?;
        if stored_digest != bundle_digest {
            return Err(TaskStoreError::MigrationBundleChanged);
        }
        let links = {
            let mut statement = transaction.prepare(
                "SELECT link.worker_id, link.imported_profile_digest,
                        profile.name, profile.description, profile.provider,
                        profile.workspace, profile.position, profile.updated_at,
                        profile.archived_at,
                        EXISTS(SELECT 1 FROM worker_sessions session
                               WHERE session.worker_id = link.worker_id),
                        EXISTS(SELECT 1 FROM tasks task
                               WHERE task.assigned_worker_id = link.worker_id)
                 FROM migration_worker_links link
                 LEFT JOIN worker_profiles profile ON profile.id = link.worker_id
                 WHERE link.batch_id = ?1 ORDER BY link.source_worker_id",
            )?;
            statement
                .query_map([batch_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                        row.get::<_, bool>(9)?,
                        row.get::<_, bool>(10)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut removable = Vec::with_capacity(links.len());
        for (
            worker_id,
            expected,
            name,
            description,
            provider,
            workspace,
            position,
            updated_at,
            archived_at,
            has_sessions,
            has_tasks,
        ) in links
        {
            let Some((name, description, provider, workspace, position, updated_at)) = name
                .zip(description)
                .zip(provider)
                .zip(workspace)
                .zip(position)
                .zip(updated_at)
                .map(
                    |(((((name, description), provider), workspace), position), updated_at)| {
                        (name, description, provider, workspace, position, updated_at)
                    },
                )
            else {
                return Err(TaskStoreError::MigrationBatchChanged);
            };
            let provider = provider
                .parse()
                .map_err(|_| TaskStoreError::MigrationBatchChanged)?;
            if archived_at.is_some()
                || has_sessions
                || has_tasks
                || worker_profile_digest(
                    &name,
                    &description,
                    provider,
                    &workspace,
                    position,
                    updated_at,
                ) != expected
            {
                return Err(TaskStoreError::MigrationBatchChanged);
            }
            removable.push(worker_id);
        }
        for worker_id in &removable {
            transaction.execute("DELETE FROM worker_profiles WHERE id = ?1", [worker_id])?;
        }
        transaction.execute(
            "UPDATE migration_worker_batches SET rolled_back_at = ?2 WHERE id = ?1",
            params![batch_id, rolled_back_at],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(LegacyWorkerMigrationRollback {
            batch_id: batch_id.to_owned(),
            removed_workers: removable.len(),
            rolled_back_at,
        })
    }

    /// Returns active migration receipts so a safe rollback remains available
    /// after navigating away from Settings or restarting the browser.
    ///
    /// # Errors
    ///
    /// Returns an error when migration records cannot be read or contain an
    /// invalid task identifier.
    pub fn list_active_legacy_migration_receipts(
        &self,
    ) -> Result<Vec<LegacyMigrationReceipt>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT batch.id, batch.source_digest, batch.source_installation_id,
                    batch.source_snapshot_digest, batch.imported_at,
                    link.task_id, link.source_task_id
             FROM migration_batches batch
             JOIN migration_task_links link ON link.batch_id = batch.id
             WHERE batch.source_kind = 'legacy' AND batch.rolled_back_at IS NULL
             ORDER BY batch.imported_at DESC, batch.id, link.source_task_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut receipts = Vec::<LegacyMigrationReceipt>::new();
        for (batch_id, digest, installation_id, snapshot_digest, imported_at, task_id, source_id) in
            rows
        {
            if receipts
                .last()
                .is_none_or(|receipt| receipt.batch_id != batch_id)
            {
                receipts.push(LegacyMigrationReceipt {
                    batch_id,
                    bundle_digest: digest,
                    source_installation_id: installation_id,
                    source_snapshot_digest: snapshot_digest,
                    imported_task_ids: Vec::new(),
                    imported_source_ids: Vec::new(),
                    imported_at,
                });
            }
            let Some(receipt) = receipts.last_mut() else {
                return Err(TaskStoreError::InvalidMigrationBundle);
            };
            receipt.imported_task_ids.push(
                task_id
                    .parse()
                    .map_err(|_| TaskStoreError::InvalidMigrationBundle)?,
            );
            receipt.imported_source_ids.push(source_id);
        }
        Ok(receipts)
    }

    /// Produces the exact normalized plan that commit will recompute.
    ///
    /// # Errors
    ///
    /// Returns an error when the package is invalid or current task and worker
    /// state cannot be read.
    pub fn preview_legacy_task_migration(
        &self,
        bundle: &LegacyMigrationBundle,
    ) -> Result<LegacyMigrationPreview, TaskStoreError> {
        let digest = validate_bundle(bundle)?;
        let normalized = self.normalize_legacy_tasks(bundle)?;
        let records = normalized
            .iter()
            .map(|task| LegacyTaskPreview {
                source_id: task.source_id.clone(),
                title: task.title.clone(),
                source_status: task.source_status.clone(),
                target_state: task.selectable.then_some(task.state),
                priority: task.priority,
                matched_worker_id: task.worker_id,
                matched_worker_name: task.worker_name.clone(),
                disposition: task.disposition,
                selectable: task.selectable,
                warnings: task.warnings.clone(),
            })
            .collect::<Vec<_>>();
        Ok(LegacyMigrationPreview {
            bundle_digest: digest,
            source_installation_id: bundle.source.installation_id.clone(),
            selectable: records.iter().filter(|record| record.selectable).count(),
            skipped: records
                .iter()
                .filter(|record| {
                    matches!(
                        record.disposition,
                        LegacyImportDisposition::Duplicate
                            | LegacyImportDisposition::SkippedJira
                            | LegacyImportDisposition::SkippedClosed
                    )
                })
                .count(),
            invalid: records
                .iter()
                .filter(|record| record.disposition == LegacyImportDisposition::Invalid)
                .count(),
            records,
        })
    }

    /// Imports explicitly selected, revalidated records in one local transaction.
    /// No worker process is started and no assignment briefing is dispatched.
    ///
    /// # Errors
    ///
    /// Returns an error when the package or selection changed, or the selected
    /// tasks cannot be committed atomically.
    #[allow(clippy::too_many_lines)]
    pub fn commit_legacy_task_migration(
        &self,
        bundle: &LegacyMigrationBundle,
        commit: &LegacyMigrationCommit,
    ) -> Result<LegacyMigrationReceipt, TaskStoreError> {
        let digest = validate_bundle(bundle)?;
        if digest != commit.bundle_digest {
            return Err(TaskStoreError::MigrationBundleChanged);
        }
        let selected = commit
            .selected_source_ids
            .iter()
            .map(|id| id.trim().to_owned())
            .collect::<HashSet<_>>();
        if selected.is_empty() || selected.len() != commit.selected_source_ids.len() {
            return Err(TaskStoreError::InvalidMigrationSelection);
        }
        let normalized = self.normalize_legacy_tasks(bundle)?;
        let selected_tasks = normalized
            .into_iter()
            .filter(|task| selected.contains(&task.source_id))
            .collect::<Vec<_>>();
        if selected_tasks.len() != selected.len()
            || selected_tasks.iter().any(|task| !task.selectable)
        {
            return Err(TaskStoreError::InvalidMigrationSelection);
        }

        let identity = self.local_hive_identity()?;
        let batch_id = Uuid::now_v7().to_string();
        let imported_at = unix_timestamp();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO migration_batches
             (id, source_kind, source_installation_id, source_digest,
              source_snapshot_digest, format_version, imported_at)
             VALUES (?1, 'legacy', ?2, ?3, ?4, ?5, ?6)",
            params![
                batch_id,
                bundle.source.installation_id,
                digest,
                bundle.source.snapshot_digest,
                i64::from(bundle.version),
                imported_at
            ],
        )?;
        let mut imported_task_ids = Vec::with_capacity(selected_tasks.len());
        let mut imported_source_ids = Vec::with_capacity(selected_tasks.len());
        for task in selected_tasks {
            let task_id = TaskId::new();
            let position: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM tasks WHERE hive_id = ?1",
                [identity.hive.id.to_string()],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT INTO tasks
                 (id, hive_id, title, description, priority, workspace, state,
                  assigned_worker_id, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    task_id.to_string(),
                    identity.hive.id.to_string(),
                    task.title,
                    task.description,
                    task.priority.to_string(),
                    task.workspace,
                    TaskState::Draft.to_string(),
                    task.worker_id.map(|id| id.to_string()),
                    position,
                    imported_at,
                    imported_at,
                ],
            )?;
            let note = format!(
                "Staged from Legacy task {} (previous state: {}). Review before approving it for work.",
                task.source_id, task.source_status
            );
            transaction.execute(
                "INSERT INTO task_activity
                 (task_id, kind, to_state, note, actor_kind)
                 VALUES (?1, 'created', ?2, ?3, ?4)",
                params![
                    task_id.to_string(),
                    TaskState::Draft.to_string(),
                    note,
                    TaskActivityActorKind::System.to_string()
                ],
            )?;
            let activity_sequence = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO migration_task_links
                 (batch_id, source_task_id, task_id, source_status, imported_activity_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    batch_id,
                    task.source_id,
                    task_id.to_string(),
                    task.source_status,
                    activity_sequence
                ],
            )?;
            imported_task_ids.push(task_id);
            imported_source_ids.push(task.source_id);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(LegacyMigrationReceipt {
            batch_id,
            bundle_digest: digest,
            source_installation_id: bundle.source.installation_id.clone(),
            source_snapshot_digest: bundle.source.snapshot_digest.clone(),
            imported_task_ids,
            imported_source_ids,
            imported_at,
        })
    }

    /// Removes an untouched import batch atomically. Any activity after import
    /// fails closed so normal work can never be erased by rollback.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt is missing or changed, an imported task
    /// has activity, or the rollback cannot be committed.
    pub fn rollback_legacy_task_migration(
        &self,
        batch_id: &str,
        bundle_digest: &str,
    ) -> Result<LegacyMigrationRollback, TaskStoreError> {
        let batch_id = batch_id.trim();
        let bundle_digest = bundle_digest.trim();
        if batch_id.is_empty() || bundle_digest.is_empty() {
            return Err(TaskStoreError::MigrationBatchNotFound);
        }
        let rolled_back_at = unix_timestamp();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let stored_digest = transaction
            .query_row(
                "SELECT source_digest FROM migration_batches
                 WHERE id = ?1 AND rolled_back_at IS NULL",
                [batch_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::MigrationBatchNotFound)?;
        if stored_digest != bundle_digest {
            return Err(TaskStoreError::MigrationBundleChanged);
        }
        let changed: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM migration_task_links link
                 WHERE link.batch_id = ?1 AND (
                     NOT EXISTS(SELECT 1 FROM tasks task WHERE task.id = link.task_id)
                     OR COALESCE((SELECT MAX(sequence) FROM task_activity activity
                                  WHERE activity.task_id = link.task_id), 0)
                        != link.imported_activity_sequence
                 )
             )",
            [batch_id],
            |row| row.get(0),
        )?;
        if changed {
            return Err(TaskStoreError::MigrationBatchChanged);
        }
        let task_ids = {
            let mut statement = transaction.prepare(
                "SELECT task_id FROM migration_task_links WHERE batch_id = ?1 ORDER BY task_id",
            )?;
            statement
                .query_map([batch_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for task_id in &task_ids {
            transaction.execute("DELETE FROM tasks WHERE id = ?1", [task_id])?;
        }
        transaction.execute(
            "UPDATE migration_batches SET rolled_back_at = ?2 WHERE id = ?1",
            params![batch_id, rolled_back_at],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(LegacyMigrationRollback {
            batch_id: batch_id.to_owned(),
            removed_tasks: task_ids.len(),
            rolled_back_at,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn normalize_legacy_tasks(
        &self,
        bundle: &LegacyMigrationBundle,
    ) -> Result<Vec<NormalizedTask>, TaskStoreError> {
        let workers = self.list_worker_profiles()?;
        let fallback_workspace = workers
            .iter()
            .find(|worker| {
                worker.role == WorkerRole::Worker && worker.name.eq_ignore_ascii_case("Scout")
            })
            .or_else(|| {
                workers
                    .iter()
                    .find(|worker| worker.role == WorkerRole::Queen)
            })
            .map(|worker| worker.workspace.clone())
            .ok_or(TaskStoreError::InvalidMigrationBundle)?;
        let mut workers_by_name: HashMap<String, Vec<_>> = HashMap::new();
        for worker in workers
            .iter()
            .filter(|worker| worker.role == WorkerRole::Worker)
        {
            workers_by_name
                .entry(worker.name.to_lowercase())
                .or_default()
                .push(worker);
        }
        let connection = self.connection()?;
        let mut output = Vec::with_capacity(bundle.tasks.len());
        for source in &bundle.tasks {
            let source_id = source.source_id.trim().to_owned();
            let title = source.title.trim().to_owned();
            let source_status = source.status.trim().to_lowercase();
            let mut warnings = Vec::new();
            let priority = map_priority(&source.priority, &mut warnings);
            let (state, mut disposition, mut selectable) = map_state(&source_status);
            let worker = source
                .assigned_worker
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .and_then(|name| workers_by_name.get(&name.to_lowercase()))
                .and_then(|matches| (matches.len() == 1).then_some(matches[0]));
            if source.assigned_worker.is_some() && worker.is_none() {
                warnings.push(
                    "Legacy worker could not be matched exactly; task will be unassigned.".into(),
                );
                if selectable && disposition == LegacyImportDisposition::Ready {
                    disposition = LegacyImportDisposition::Transformed;
                }
            }
            let workspace = worker.map_or_else(
                || fallback_workspace.clone(),
                |worker| worker.workspace.clone(),
            );
            let description = build_description(source);

            if source
                .jira_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
            {
                disposition = LegacyImportDisposition::SkippedJira;
                selectable = false;
                warnings.push(
                    "Jira remains canonical; this issue will return through Jira sync.".into(),
                );
            }
            if source_id.is_empty()
                || title.is_empty()
                || title.len() > MAX_TASK_TITLE_BYTES
                || description.len() > MAX_TASK_DESCRIPTION_BYTES
            {
                disposition = LegacyImportDisposition::Invalid;
                selectable = false;
                warnings.push("Required text is empty or exceeds Swarm Next safety bounds.".into());
            }
            if source.attachment_count > 0 {
                warnings.push(format!(
                    "{} Legacy attachment(s) are not copied in this migration slice.",
                    source.attachment_count
                ));
                if selectable && disposition == LegacyImportDisposition::Ready {
                    disposition = LegacyImportDisposition::Transformed;
                }
            }
            if source.source_email_id.is_some() {
                warnings.push(
                    "The original email reply identity is retained only in the migration package."
                        .into(),
                );
                if selectable && disposition == LegacyImportDisposition::Ready {
                    disposition = LegacyImportDisposition::Transformed;
                }
            }
            let already_imported = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM migration_batches batch
                 JOIN migration_task_links link ON link.batch_id = batch.id
                 WHERE batch.source_installation_id = ?1 AND link.source_task_id = ?2
                   AND batch.rolled_back_at IS NULL)",
                params![bundle.source.installation_id, source_id],
                |row| row.get::<_, bool>(0),
            )?;
            let matching_task = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks
                 WHERE lower(trim(title)) = lower(trim(?1)) AND workspace = ?2
                   AND state != 'completed')",
                params![title, workspace],
                |row| row.get::<_, bool>(0),
            )?;
            if already_imported || matching_task {
                disposition = LegacyImportDisposition::Duplicate;
                selectable = false;
                warnings.push(if already_imported {
                    "This exact Legacy record is already present in an active migration batch."
                        .into()
                } else {
                    "An open task with the same title and workspace already exists.".into()
                });
            }
            if source_status == "active" {
                warnings.push(
                    "Legacy Active becomes Ready because no running Legacy process is transferred."
                        .into(),
                );
                if selectable {
                    disposition = LegacyImportDisposition::Transformed;
                }
            }
            output.push(NormalizedTask {
                source_id,
                title,
                description,
                source_status,
                state,
                priority,
                workspace,
                worker_id: worker.map(|worker| worker.id),
                worker_name: worker.map(|worker| worker.name.clone()),
                disposition,
                selectable,
                warnings,
            });
        }
        Ok(output)
    }

    #[allow(clippy::too_many_lines)]
    fn normalize_legacy_workers(
        &self,
        bundle: &LegacyMigrationBundle,
    ) -> Result<Vec<NormalizedWorker>, TaskStoreError> {
        let existing = self.list_worker_profiles()?;
        let home = migration_home_directory();
        let connection = self.connection()?;
        let mut output = Vec::with_capacity(bundle.workers.len());
        for source in &bundle.workers {
            let source_id = source.source_id.trim().to_owned();
            let name = source.name.trim().to_owned();
            let workspace = source.workspace.trim().to_owned();
            let description = source.description.trim().to_owned();
            let mut warnings = Vec::new();
            let mut disposition = LegacyWorkerImportDisposition::Ready;
            let mut selectable = true;
            let provider = match source.provider.trim().to_lowercase().as_str() {
                "codex" => ProviderKind::Codex,
                "" | "claude" | "claude_code" => ProviderKind::ClaudeCode,
                other => {
                    warnings.push(format!(
                        "Legacy provider '{other}' is unavailable; this worker will use Claude Code."
                    ));
                    disposition = LegacyWorkerImportDisposition::Transformed;
                    ProviderKind::ClaudeCode
                }
            };
            let provider_conversation_id = source
                .provider_conversation_id
                .as_deref()
                .map(str::parse)
                .transpose()
                .ok()
                .flatten();
            if source.provider_conversation_id.is_some() && provider_conversation_id.is_none() {
                warnings.push(
                    "Legacy reported a provider conversation with an invalid identity; this worker will start fresh."
                        .into(),
                );
                disposition = LegacyWorkerImportDisposition::Transformed;
            } else if provider_conversation_id.is_none() {
                warnings.push(
                    "No exact Legacy conversation was found for this repository; this worker will start fresh."
                        .into(),
                );
            }
            if source.has_identity_file {
                warnings.push(
                    "Legacy identity-file content is private and is not imported; use the reviewed worker description instead."
                        .into(),
                );
                disposition = LegacyWorkerImportDisposition::Transformed;
            }
            if !source.isolation.trim().is_empty() {
                warnings.push(
                    "Legacy isolation settings are not copied; Swarm Next applies its own workspace policy."
                        .into(),
                );
                disposition = LegacyWorkerImportDisposition::Transformed;
            }
            if name.eq_ignore_ascii_case("queen")
                || name.eq_ignore_ascii_case("project root")
                || name.eq_ignore_ascii_case("scout")
            {
                disposition = LegacyWorkerImportDisposition::ManagedByNext;
                selectable = false;
                warnings.push(if name.eq_ignore_ascii_case("queen") {
                    "Swarm Next owns one durable Queen; the Legacy Queen is not duplicated.".into()
                } else {
                    "Swarm Next Scout owns cross-repository work; Project Root is not duplicated."
                        .into()
                });
            }
            if source_id.is_empty()
                || workspace.is_empty()
                || workspace.len() > super::MAX_WORKSPACE_BYTES
                || workspace.chars().any(char::is_control)
                || super::workers::validate_worker_name(&name).is_err()
                || super::workers::validate_worker_description(&description).is_err()
            {
                disposition = LegacyWorkerImportDisposition::Invalid;
                selectable = false;
                warnings.push(
                    "Required worker data is empty, contains control characters, or exceeds Swarm Next safety bounds."
                        .into(),
                );
            }
            let already_imported = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM migration_worker_batches batch
                 JOIN migration_worker_links link ON link.batch_id = batch.id
                 WHERE batch.source_installation_id = ?1 AND link.source_worker_id = ?2
                   AND batch.rolled_back_at IS NULL)",
                params![bundle.source.installation_id, source_id],
                |row| row.get::<_, bool>(0),
            )?;
            let duplicate = existing.iter().find(|worker| {
                worker.name.eq_ignore_ascii_case(&name)
                    || workspace_identity(&worker.workspace, home.as_deref())
                        == workspace_identity(&workspace, home.as_deref())
            });
            if selectable && (already_imported || duplicate.is_some()) {
                disposition = LegacyWorkerImportDisposition::Duplicate;
                selectable = false;
                warnings.push(if already_imported {
                    "This exact Legacy worker is already present in an active migration batch."
                        .into()
                } else if let Some(duplicate) = duplicate {
                    format!(
                        "Swarm Next already has worker '{}' for this name or repository.",
                        duplicate.name
                    )
                } else {
                    "Swarm Next already has this worker.".into()
                });
            }
            output.push(NormalizedWorker {
                source_id,
                name,
                workspace,
                description,
                provider,
                provider_conversation_id,
                disposition,
                selectable,
                warnings,
            });
        }
        Ok(output)
    }
}

fn migration_home_directory() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .map(|home| home.replace('\\', "/").trim_end_matches('/').to_owned())
        .filter(|home| !home.is_empty())
}

fn workspace_identity(workspace: &str, home: Option<&str>) -> String {
    let normalized = workspace.trim().replace('\\', "/");
    let mut expanded = match (normalized.strip_prefix("~/"), home) {
        (Some(relative), Some(home)) => format!("{home}/{relative}"),
        _ => normalized,
    };
    while expanded.contains("//") {
        expanded = expanded.replace("//", "/");
    }
    if expanded.len() > 1 {
        expanded = expanded.trim_end_matches('/').to_owned();
    }
    if expanded.as_bytes().get(1) == Some(&b':') {
        expanded.make_ascii_lowercase();
    }
    expanded
}

fn worker_profile_digest(
    name: &str,
    description: &str,
    provider: ProviderKind,
    workspace: &str,
    position: i64,
    updated_at: i64,
) -> String {
    let bytes = serde_json::to_vec(&(
        name,
        description,
        provider.to_string(),
        workspace,
        position,
        updated_at,
    ))
    .expect("worker profile digest tuple serializes");
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_bundle(bundle: &LegacyMigrationBundle) -> Result<String, TaskStoreError> {
    if bundle.format != LEGACY_MIGRATION_FORMAT
        || bundle.version != LEGACY_MIGRATION_VERSION
        || bundle.tasks.len() > MAX_MIGRATION_TASKS
        || bundle.workers.len() > MAX_MIGRATION_WORKERS
        || bundle.source.installation_id.trim().is_empty()
        || bundle.source.snapshot_digest.trim().is_empty()
    {
        return Err(TaskStoreError::InvalidMigrationBundle);
    }
    let bytes = serde_json::to_vec(bundle).map_err(|_| TaskStoreError::InvalidMigrationBundle)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn map_state(status: &str) -> (TaskState, LegacyImportDisposition, bool) {
    match status {
        "backlog" => (TaskState::Draft, LegacyImportDisposition::Ready, true),
        "unassigned" | "assigned" => (TaskState::Ready, LegacyImportDisposition::Ready, true),
        "active" => (TaskState::Ready, LegacyImportDisposition::Transformed, true),
        "blocked" => (TaskState::Blocked, LegacyImportDisposition::Ready, true),
        "done" | "failed" | "completed" => (
            TaskState::Completed,
            LegacyImportDisposition::SkippedClosed,
            false,
        ),
        _ => (TaskState::Draft, LegacyImportDisposition::Invalid, false),
    }
}

fn map_priority(priority: &str, warnings: &mut Vec<String>) -> TaskPriority {
    match priority.trim().to_lowercase().as_str() {
        "low" => TaskPriority::Low,
        "high" => TaskPriority::High,
        "urgent" | "critical" => TaskPriority::Urgent,
        "" | "normal" | "medium" => TaskPriority::Normal,
        _ => {
            warnings.push("Unknown Legacy priority became Normal.".into());
            TaskPriority::Normal
        }
    }
}

fn build_description(source: &LegacyTaskRecord) -> String {
    let mut sections = Vec::new();
    if !source.description.trim().is_empty() {
        sections.push(source.description.trim().to_owned());
    }
    if let Some(reason) = source
        .block_reason
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        sections.push(format!("Blocked because: {reason}"));
    }
    if !source.acceptance_criteria.is_empty() {
        sections.push(format!(
            "Acceptance criteria:\n{}",
            source
                .acceptance_criteria
                .iter()
                .map(|criterion| format!("- {}", criterion.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.join("\n\n")
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().cast_signed())
}

pub(super) fn migrate_legacy_migration_batches(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS migration_batches (
             id TEXT PRIMARY KEY,
             source_kind TEXT NOT NULL CHECK (source_kind IN ('legacy')),
             source_installation_id TEXT NOT NULL,
             source_digest TEXT NOT NULL,
             source_snapshot_digest TEXT NOT NULL,
             format_version INTEGER NOT NULL,
             imported_at INTEGER NOT NULL,
             rolled_back_at INTEGER
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_active_migration_per_source_digest
             ON migration_batches(source_installation_id, source_digest)
             WHERE rolled_back_at IS NULL;
         CREATE TABLE IF NOT EXISTS migration_task_links (
             batch_id TEXT NOT NULL REFERENCES migration_batches(id) ON DELETE CASCADE,
             source_task_id TEXT NOT NULL,
             task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
             source_status TEXT NOT NULL,
             imported_activity_sequence INTEGER NOT NULL,
             PRIMARY KEY (batch_id, source_task_id)
         );
         CREATE INDEX IF NOT EXISTS migration_task_source_lookup
             ON migration_task_links(source_task_id, batch_id);
         PRAGMA user_version = 67;",
    )
}

pub(super) fn migrate_legacy_worker_migrations(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS migration_worker_batches (
             id TEXT PRIMARY KEY,
             source_installation_id TEXT NOT NULL,
             source_digest TEXT NOT NULL,
             imported_at INTEGER NOT NULL,
             rolled_back_at INTEGER
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_active_worker_migration_per_source_digest
             ON migration_worker_batches(source_installation_id, source_digest)
             WHERE rolled_back_at IS NULL;
         CREATE TABLE IF NOT EXISTS migration_worker_links (
             batch_id TEXT NOT NULL REFERENCES migration_worker_batches(id) ON DELETE CASCADE,
             source_worker_id TEXT NOT NULL,
             worker_id TEXT NOT NULL UNIQUE REFERENCES worker_profiles(id) ON DELETE CASCADE,
             imported_profile_digest TEXT NOT NULL,
             PRIMARY KEY (batch_id, source_worker_id)
         );
         CREATE INDEX IF NOT EXISTS migration_worker_source_lookup
             ON migration_worker_links(source_worker_id, batch_id);
         PRAGMA user_version = 68;",
    )
}

pub(super) fn migrate_legacy_provider_conversations(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let worker_profiles_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get(0),
    )?;
    if worker_profiles_exist {
        let resume_column_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_profiles')
                           WHERE name = 'provider_conversation_resume')",
            [],
            |row| row.get(0),
        )?;
        if !resume_column_exists {
            transaction.execute_batch(
                "ALTER TABLE worker_profiles
                 ADD COLUMN provider_conversation_resume INTEGER NOT NULL DEFAULT 0
                 CHECK (provider_conversation_resume IN (0, 1));",
            )?;
        }
    }

    let migration_links_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'migration_worker_links')",
        [],
        |row| row.get(0),
    )?;
    if migration_links_exist {
        let resumed_column_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('migration_worker_links')
                           WHERE name = 'resumed_conversation')",
            [],
            |row| row.get(0),
        )?;
        if !resumed_column_exists {
            transaction.execute_batch(
                "ALTER TABLE migration_worker_links
                 ADD COLUMN resumed_conversation INTEGER NOT NULL DEFAULT 0
                 CHECK (resumed_conversation IN (0, 1));",
            )?;
        }
    }

    transaction.pragma_update(None, "user_version", 71)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    fn bundle(tasks: Vec<LegacyTaskRecord>) -> LegacyMigrationBundle {
        LegacyMigrationBundle {
            format: LEGACY_MIGRATION_FORMAT.into(),
            version: LEGACY_MIGRATION_VERSION,
            source: LegacyMigrationSource {
                installation_id: "legacy-hive-a".into(),
                schema_version: Some(21),
                exported_at: 1_700_000_000,
                snapshot_digest: "abc123".into(),
            },
            tasks,
            workers: Vec::new(),
        }
    }

    fn task(id: &str, status: &str) -> LegacyTaskRecord {
        LegacyTaskRecord {
            source_id: id.into(),
            title: format!("Legacy {id}"),
            description: "A preserved outcome".into(),
            status: status.into(),
            priority: "high".into(),
            assigned_worker: Some("Clover".into()),
            jira_key: None,
            block_reason: None,
            acceptance_criteria: vec!["Verified".into()],
            attachment_count: 0,
            source_email_id: None,
            created_at: Some(100),
            updated_at: Some(200),
        }
    }

    fn worker(id: &str, name: &str, workspace: &str) -> LegacyWorkerRecord {
        LegacyWorkerRecord {
            source_id: id.into(),
            name: name.into(),
            workspace: workspace.into(),
            description: format!("Owns {name}"),
            provider: "claude".into(),
            position: 4,
            has_identity_file: false,
            isolation: String::new(),
            provider_conversation_id: None,
        }
    }

    fn worker_bundle(workers: Vec<LegacyWorkerRecord>) -> LegacyMigrationBundle {
        let mut bundle = bundle(Vec::new());
        bundle.workers = workers;
        bundle
    }

    fn store() -> TaskStore {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/projects").unwrap();
        store
            .create_worker("Scout", ProviderKind::ClaudeCode, "/projects", false, 1)
            .unwrap();
        store
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/projects/clover",
                false,
                2,
            )
            .unwrap();
        store
    }

    #[test]
    fn preview_excludes_jira_and_transforms_active_without_dispatching() {
        let store = store();
        let mut jira = task("jira", "assigned");
        jira.jira_key = Some("WWD-1".into());
        let preview = store
            .preview_legacy_task_migration(&bundle(vec![task("active", "active"), jira]))
            .unwrap();
        assert_eq!(preview.selectable, 1);
        assert_eq!(preview.skipped, 1);
        assert_eq!(preview.records[0].target_state, Some(TaskState::Ready));
        assert_eq!(
            preview.records[0].matched_worker_name.as_deref(),
            Some("Clover")
        );
        assert_eq!(
            preview.records[1].disposition,
            LegacyImportDisposition::SkippedJira
        );
    }

    #[test]
    fn commit_is_atomic_provenanced_and_duplicate_safe() {
        let store = store();
        let bundle = bundle(vec![task("one", "assigned"), task("two", "blocked")]);
        let preview = store.preview_legacy_task_migration(&bundle).unwrap();
        let receipt = store
            .commit_legacy_task_migration(
                &bundle,
                &LegacyMigrationCommit {
                    bundle_digest: preview.bundle_digest,
                    selected_source_ids: vec!["one".into(), "two".into()],
                },
            )
            .unwrap();
        assert_eq!(receipt.imported_task_ids.len(), 2);
        assert_eq!(receipt.source_installation_id, "legacy-hive-a");
        assert_eq!(receipt.source_snapshot_digest, "abc123");
        assert_eq!(
            store.list_active_legacy_migration_receipts().unwrap(),
            vec![receipt.clone()]
        );
        let imported = store.list_tasks().unwrap();
        assert_eq!(imported.len(), 2);
        assert!(
            imported
                .iter()
                .all(|task| task.assigned_session_id.is_none())
        );
        assert!(imported.iter().all(|task| task.state == TaskState::Draft));
        let second = store.preview_legacy_task_migration(&bundle).unwrap();
        assert_eq!(second.selectable, 0);
        assert!(
            second
                .records
                .iter()
                .all(|record| record.disposition == LegacyImportDisposition::Duplicate)
        );
    }

    #[test]
    fn commit_rejects_a_changed_preview_digest() {
        let store = store();
        let bundle = bundle(vec![task("one", "assigned")]);
        let error = store
            .commit_legacy_task_migration(
                &bundle,
                &LegacyMigrationCommit {
                    bundle_digest: "changed".into(),
                    selected_source_ids: vec!["one".into()],
                },
            )
            .unwrap_err();
        assert!(matches!(error, TaskStoreError::MigrationBundleChanged));
        assert!(store.list_tasks().unwrap().is_empty());
    }

    #[test]
    fn rollback_removes_only_an_untouched_batch() {
        let store = store();
        let bundle = bundle(vec![task("one", "assigned")]);
        let preview = store.preview_legacy_task_migration(&bundle).unwrap();
        let receipt = store
            .commit_legacy_task_migration(
                &bundle,
                &LegacyMigrationCommit {
                    bundle_digest: preview.bundle_digest.clone(),
                    selected_source_ids: vec!["one".into()],
                },
            )
            .unwrap();
        let rollback = store
            .rollback_legacy_task_migration(&receipt.batch_id, &preview.bundle_digest)
            .unwrap();
        assert_eq!(rollback.removed_tasks, 1);
        assert!(store.list_tasks().unwrap().is_empty());
        assert!(
            store
                .list_active_legacy_migration_receipts()
                .unwrap()
                .is_empty()
        );

        let preview = store.preview_legacy_task_migration(&bundle).unwrap();
        let receipt = store
            .commit_legacy_task_migration(
                &bundle,
                &LegacyMigrationCommit {
                    bundle_digest: preview.bundle_digest.clone(),
                    selected_source_ids: vec!["one".into()],
                },
            )
            .unwrap();
        store
            .update_task_details_as(
                receipt.imported_task_ids[0],
                &swarm_domain::TaskDetailsUpdate {
                    title: Some("Changed safely".into()),
                    ..Default::default()
                },
                &swarm_domain::TaskActivityActor::operator(),
            )
            .unwrap();
        assert!(matches!(
            store.rollback_legacy_task_migration(&receipt.batch_id, &preview.bundle_digest),
            Err(TaskStoreError::MigrationBatchChanged)
        ));
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn worker_preview_skips_managed_and_duplicate_profiles() {
        let store = store();
        let bundle = worker_bundle(vec![
            worker("new", "Daisy", "/projects/daisy"),
            worker("root", "Project Root", "/projects"),
            worker("duplicate", "Clover", "/projects/clover-other"),
        ]);

        let preview = store.preview_legacy_worker_migration(&bundle).unwrap();

        assert_eq!(preview.selectable, 1);
        assert_eq!(preview.skipped, 2);
        assert_eq!(preview.invalid, 0);
        assert!(preview.records[0].selectable);
        assert_eq!(
            preview.records[1].disposition,
            LegacyWorkerImportDisposition::ManagedByNext
        );
        assert_eq!(
            preview.records[2].disposition,
            LegacyWorkerImportDisposition::Duplicate
        );
    }

    #[test]
    fn worker_preview_treats_tilde_and_absolute_home_repositories_as_duplicates() {
        let store = store();
        let home = migration_home_directory().expect("test process has a home directory");
        store
            .create_worker(
                "Daisy",
                ProviderKind::ClaudeCode,
                &format!("{home}/projects/daisy"),
                false,
                3,
            )
            .unwrap();

        let preview = store
            .preview_legacy_worker_migration(&worker_bundle(vec![worker(
                "daisy",
                "Legacy Daisy",
                "~/projects/daisy/",
            )]))
            .unwrap();

        assert_eq!(preview.selectable, 0);
        assert_eq!(preview.skipped, 1);
        assert_eq!(
            preview.records[0].disposition,
            LegacyWorkerImportDisposition::Duplicate
        );
        assert!(preview.records[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("Daisy")));
    }

    #[test]
    fn worker_import_is_sleeping_ordered_provenanced_and_reversible() {
        let store = store();
        let bundle = worker_bundle(vec![
            worker("daisy", "Daisy", "/projects/daisy"),
            LegacyWorkerRecord {
                provider: "unknown-provider".into(),
                has_identity_file: true,
                ..worker("poppy", "Poppy", "/projects/poppy")
            },
        ]);
        let preview = store.preview_legacy_worker_migration(&bundle).unwrap();
        assert_eq!(preview.selectable, 2);
        assert_eq!(
            preview.records[1].disposition,
            LegacyWorkerImportDisposition::Transformed
        );
        let receipt = store
            .commit_legacy_worker_migration(
                &bundle,
                &LegacyWorkerMigrationCommit {
                    bundle_digest: preview.bundle_digest.clone(),
                    selected_source_ids: vec!["daisy".into(), "poppy".into()],
                    resume_legacy_conversations: false,
                },
            )
            .unwrap();
        assert_eq!(receipt.imported_worker_ids.len(), 2);
        let profiles = store.list_worker_profiles().unwrap();
        let daisy = profiles
            .iter()
            .find(|worker| worker.name == "Daisy")
            .unwrap();
        let poppy = profiles
            .iter()
            .find(|worker| worker.name == "Poppy")
            .unwrap();
        assert!(daisy.active_session_id.is_none());
        assert!(!daisy.autostart);
        assert_eq!(poppy.provider, ProviderKind::ClaudeCode);
        assert!(daisy.position < poppy.position);
        assert_eq!(
            store
                .list_active_legacy_worker_migration_receipts()
                .unwrap(),
            vec![receipt.clone()]
        );

        let rollback = store
            .rollback_legacy_worker_migration(&receipt.batch_id, &receipt.bundle_digest)
            .unwrap();
        assert_eq!(rollback.removed_workers, 2);
        assert!(
            store
                .list_worker_profiles()
                .unwrap()
                .iter()
                .all(|worker| worker.name != "Daisy" && worker.name != "Poppy")
        );
    }

    #[test]
    fn worker_import_option_preserves_exact_provider_conversation_for_first_wake() {
        let store = store();
        let conversation_id = uuid::Uuid::now_v7().to_string();
        let bundle = worker_bundle(vec![LegacyWorkerRecord {
            provider_conversation_id: Some(conversation_id.clone()),
            ..worker("daisy", "Daisy", "/projects/daisy")
        }]);
        let preview = store.preview_legacy_worker_migration(&bundle).unwrap();
        assert!(preview.records[0].conversation_available);

        let receipt = store
            .commit_legacy_worker_migration(
                &bundle,
                &LegacyWorkerMigrationCommit {
                    bundle_digest: preview.bundle_digest,
                    selected_source_ids: vec!["daisy".into()],
                    resume_legacy_conversations: true,
                },
            )
            .unwrap();
        assert_eq!(receipt.resumed_source_ids, ["daisy"]);
        let imported = store
            .get_worker_profile(receipt.imported_worker_ids[0])
            .unwrap();
        assert_eq!(
            imported
                .provider_conversation_id
                .map(|value| value.to_string()),
            Some(conversation_id)
        );
        assert!(imported.has_session_history);
        assert!(imported.active_session_id.is_none());
    }

    #[test]
    fn worker_rollback_refuses_profile_changes_or_use() {
        let store = store();
        let bundle = worker_bundle(vec![worker("daisy", "Daisy", "/projects/daisy")]);
        let preview = store.preview_legacy_worker_migration(&bundle).unwrap();
        let receipt = store
            .commit_legacy_worker_migration(
                &bundle,
                &LegacyWorkerMigrationCommit {
                    bundle_digest: preview.bundle_digest,
                    selected_source_ids: vec!["daisy".into()],
                    resume_legacy_conversations: false,
                },
            )
            .unwrap();
        store
            .update_worker_profile(
                receipt.imported_worker_ids[0],
                None,
                Some("Reviewed owner description"),
                None,
                None,
            )
            .unwrap();

        assert!(matches!(
            store.rollback_legacy_worker_migration(&receipt.batch_id, &receipt.bundle_digest),
            Err(TaskStoreError::MigrationBatchChanged)
        ));
    }
}
