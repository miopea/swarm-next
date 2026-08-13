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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CodexConversationStart {
    New,
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
    #[error("MCP configuration path must be valid UTF-8")]
    McpConfigNotUtf8,
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
    ) -> Result<ProviderCommand, ProviderCommandError> {
        self.command_for_with_mcp(workspace, conversation, None)
    }

    /// Builds a provider command with an optional explicit MCP configuration.
    ///
    /// # Errors
    /// Returns an error for a relative workspace or a non-UTF-8 MCP configuration path.
    fn command_for_with_mcp(
        &self,
        workspace: &Path,
        conversation: ClaudeConversationStart,
        mcp_config: Option<&Path>,
    ) -> Result<ProviderCommand, ProviderCommandError>;
}

#[derive(Clone, Debug, Default)]
pub struct ClaudeCodeAdapter;

impl ProviderTerminalAdapter for ClaudeCodeAdapter {
    fn command_for_with_mcp(
        &self,
        workspace: &Path,
        conversation: ClaudeConversationStart,
        mcp_config: Option<&Path>,
    ) -> Result<ProviderCommand, ProviderCommandError> {
        if !workspace.is_absolute() {
            return Err(ProviderCommandError::WorkspaceNotAbsolute);
        }
        let mut arguments = match conversation {
            ClaudeConversationStart::New { session_id } => {
                vec!["--session-id".into(), session_id.to_string()]
            }
            ClaudeConversationStart::Resume { session_id } => {
                vec!["--resume".into(), session_id.to_string()]
            }
            ClaudeConversationStart::Continue => vec!["--continue".into()],
        };
        if let Some(config) = mcp_config {
            let config = config
                .to_str()
                .ok_or(ProviderCommandError::McpConfigNotUtf8)?;
            arguments.push("--mcp-config".into());
            arguments.push(config.into());
        }
        Ok(ProviderCommand {
            executable: PathBuf::from("claude"),
            arguments,
            working_directory: workspace.to_path_buf(),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct CodexAdapter;

impl CodexAdapter {
    /// Builds the interactive Codex CLI command for a repository-owned worker.
    /// Codex owns its thread identifier; recovery therefore uses its cwd-scoped
    /// `resume --last` contract instead of manufacturing an external UUID.
    /// # Errors
    /// Returns an error when the workspace is not an allowed absolute path.
    pub fn command_for(
        &self,
        workspace: &Path,
        conversation: CodexConversationStart,
    ) -> Result<ProviderCommand, ProviderCommandError> {
        if !workspace.is_absolute() {
            return Err(ProviderCommandError::WorkspaceNotAbsolute);
        }
        let arguments = match conversation {
            CodexConversationStart::New => Vec::new(),
            CodexConversationStart::Continue => vec!["resume".into(), "--last".into()],
        };
        Ok(ProviderCommand {
            executable: PathBuf::from("codex"),
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
    fn claude_loads_the_private_mcp_config_in_every_recovery_mode() {
        let config = Path::new("/state/swarm-next/agents/worker.json");
        for start in [
            ClaudeConversationStart::New {
                session_id: ProviderConversationId::new(),
            },
            ClaudeConversationStart::Resume {
                session_id: ProviderConversationId::new(),
            },
            ClaudeConversationStart::Continue,
        ] {
            let command = ClaudeCodeAdapter
                .command_for_with_mcp(Path::new("/workspace/example"), start, Some(config))
                .unwrap();
            assert_eq!(
                &command.arguments[command.arguments.len() - 2..],
                ["--mcp-config", "/state/swarm-next/agents/worker.json"]
            );
        }
    }
    #[test]
    fn relative_workspace_fails_closed() {
        assert_eq!(
            ClaudeCodeAdapter
                .command_for(Path::new("relative"), ClaudeConversationStart::Continue,),
            Err(ProviderCommandError::WorkspaceNotAbsolute)
        );
    }

    #[test]
    fn codex_starts_new_and_recovers_the_latest_repository_thread() {
        let adapter = CodexAdapter;
        let fresh = adapter
            .command_for(
                Path::new("/workspaces/example"),
                CodexConversationStart::New,
            )
            .unwrap();
        assert_eq!(fresh.executable, PathBuf::from("codex"));
        assert!(fresh.arguments.is_empty());
        let recovered = adapter
            .command_for(
                Path::new("/workspaces/example"),
                CodexConversationStart::Continue,
            )
            .unwrap();
        assert_eq!(recovered.arguments, ["resume", "--last"]);
    }
}
