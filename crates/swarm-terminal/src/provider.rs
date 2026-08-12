use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use swarm_domain::ProviderConversationId;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ClaudeConversationStart {
    New { session_id: ProviderConversationId },
    Resume { session_id: ProviderConversationId },
    Continue,
}

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
    fn command_for(
        &self,
        workspace: &Path,
        conversation: ClaudeConversationStart,
    ) -> Result<ProviderCommand, ProviderCommandError>;
}

#[derive(Clone, Debug, Default)]
pub struct ClaudeCodeAdapter;

impl ProviderTerminalAdapter for ClaudeCodeAdapter {
    fn command_for(
        &self,
        workspace: &Path,
        conversation: ClaudeConversationStart,
    ) -> Result<ProviderCommand, ProviderCommandError> {
        if !workspace.is_absolute() {
            return Err(ProviderCommandError::WorkspaceNotAbsolute);
        }
        let arguments = match conversation {
            ClaudeConversationStart::New { session_id } => {
                vec!["--session-id".into(), session_id.to_string()]
            }
            ClaudeConversationStart::Resume { session_id } => {
                vec!["--resume".into(), session_id.to_string()]
            }
            ClaudeConversationStart::Continue => vec!["--continue".into()],
        };
        Ok(ProviderCommand {
            executable: PathBuf::from("claude"),
            arguments,
            working_directory: workspace.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_starts_a_new_exact_conversation_without_overriding_permissions() {
        let session_id = ProviderConversationId::new();
        let command = ClaudeCodeAdapter
            .command_for(
                Path::new("/workspaces/example"),
                ClaudeConversationStart::New { session_id },
            )
            .expect("absolute workspace should be valid");
        assert_eq!(command.executable, PathBuf::from("claude"));
        assert_eq!(command.arguments, ["--session-id", &session_id.to_string()]);
    }

    #[test]
    fn claude_resumes_an_exact_conversation_when_known() {
        let session_id = ProviderConversationId::new();
        let command = ClaudeCodeAdapter
            .command_for(
                Path::new("/workspaces/example"),
                ClaudeConversationStart::Resume { session_id },
            )
            .unwrap();
        assert_eq!(command.arguments, ["--resume", &session_id.to_string()]);
    }

    #[test]
    fn claude_continues_the_workspace_for_migrated_profiles() {
        let command = ClaudeCodeAdapter
            .command_for(
                Path::new("/workspaces/example"),
                ClaudeConversationStart::Continue,
            )
            .unwrap();
        assert_eq!(command.arguments, ["--continue"]);
    }

    #[test]
    fn relative_workspace_fails_closed() {
        assert_eq!(
            ClaudeCodeAdapter
                .command_for(Path::new("relative"), ClaudeConversationStart::Continue,),
            Err(ProviderCommandError::WorkspaceNotAbsolute)
        );
    }
}
