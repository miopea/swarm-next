use swarm_domain::WorkspaceSearchSettings;
use swarm_persistence::{TaskStore, TaskStoreError};

#[derive(Clone)]
pub struct WorkspaceSettingsService(pub TaskStore);

impl WorkspaceSettingsService {
    /// # Errors
    /// Returns persistence failures.
    pub fn settings(&self) -> Result<WorkspaceSearchSettings, TaskStoreError> {
        let settings = self.0.workspace_search_settings()?;
        if !WorkspaceSearchSettings::valid_folders(&settings.folders) {
            return Err(TaskStoreError::IntegrityFailure(
                "invalid saved workspace search settings".into(),
            ));
        }
        Ok(settings)
    }

    /// Replaces the operator-approved canonical discovery folders.
    /// # Errors
    /// Returns validation or persistence errors. False means the editor is stale.
    pub fn replace(&self, revision: u64, folders: &[String]) -> Result<bool, TaskStoreError> {
        if !WorkspaceSearchSettings::valid_folders(folders) {
            return Err(TaskStoreError::IntegrityFailure(
                "Choose at most 16 distinct repository search folders.".into(),
            ));
        }
        self.0.replace_workspace_search_settings(revision, folders)
    }
}
