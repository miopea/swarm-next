use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCommand {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProviderCommandError {
    #[error("workspace must be an absolute path")]
    WorkspaceNotAbsolute,
}

pub trait ProviderTerminalAdapter {
    /// Builds the provider command for an absolute workspace path.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderCommandError::WorkspaceNotAbsolute`] when the caller
    /// has not resolved the workspace through the trusted workspace boundary.
    fn command_for(&self, workspace: &Path) -> Result<ProviderCommand, ProviderCommandError>;
}

#[derive(Clone, Debug, Default)]
pub struct ClaudeCodeAdapter;

impl ProviderTerminalAdapter for ClaudeCodeAdapter {
    fn command_for(&self, workspace: &Path) -> Result<ProviderCommand, ProviderCommandError> {
        if !workspace.is_absolute() {
            return Err(ProviderCommandError::WorkspaceNotAbsolute);
        }
        Ok(ProviderCommand {
            executable: PathBuf::from("claude"),
            arguments: vec!["--permission-mode".into(), "auto".into()],
            working_directory: workspace.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_uses_provider_owned_auto_permission_mode() {
        let command = ClaudeCodeAdapter
            .command_for(Path::new("/workspaces/example"))
            .expect("absolute workspace should be valid");
        assert_eq!(command.executable, PathBuf::from("claude"));
        assert_eq!(command.arguments, ["--permission-mode", "auto"]);
    }

    #[test]
    fn relative_workspace_fails_closed() {
        assert_eq!(
            ClaudeCodeAdapter.command_for(Path::new("relative")),
            Err(ProviderCommandError::WorkspaceNotAbsolute)
        );
    }
}
