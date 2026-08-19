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
    #[error("MCP configuration path must be valid UTF-8")]
    McpConfigNotUtf8,
    #[error("Claude settings path must be valid UTF-8")]
    ClaudeSettingsNotUtf8,
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

impl ClaudeCodeAdapter {
    /// Builds a Claude command that keeps Swarm's isolated session profile while
    /// layering the operator's machine settings onto the provider process.
    ///
    /// # Errors
    /// Returns an error for invalid workspace, MCP, or settings paths.
    pub fn command_for_with_configuration(
        &self,
        workspace: &Path,
        conversation: ClaudeConversationStart,
        mcp_config: Option<&Path>,
        settings: Option<&Path>,
    ) -> Result<ProviderCommand, ProviderCommandError> {
        let mut command = self.command_for_with_mcp(workspace, conversation, mcp_config)?;
        if let Some(settings) = settings {
            let settings = settings
                .to_str()
                .ok_or(ProviderCommandError::ClaudeSettingsNotUtf8)?;
            command.arguments.push("--settings".into());
            command.arguments.push(settings.into());
        }
        Ok(command)
    }
}

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
            // Worker authority is deliberately defined by the role-scoped
            // Swarm config. Loading account or project MCP servers alongside
            // it can hide the required tools after a resumed conversation and
            // gives unattended workers capabilities the Hive did not grant.
            arguments.push("--strict-mcp-config".into());
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
    /// Codex owns its thread identifier. Exact imported identities are resumed
    /// directly; ordinary recovery uses its cwd-scoped `resume --last` contract.
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
            CodexConversationStart::Resume { session_id } => {
                vec!["resume".into(), session_id.to_string()]
            }
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
    fn a_provider_release_is_the_versioned_file_a_symlink_points_at() {
        // Claude installs each release beside the others and moves a symlink,
        // so a worker started before an update keeps running the older file
        // until it restarts. This is what tells those two apart.
        let root = tempfile::TempDir::new().unwrap();
        let versions = root.path().join("versions");
        std::fs::create_dir_all(&versions).unwrap();
        let older = versions.join("2.1.235");
        let newer = versions.join("2.1.236");
        std::fs::write(&older, b"old").unwrap();
        std::fs::write(&newer, b"new").unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let link = bin.join("claude");
        std::os::unix::fs::symlink(&older, &link).unwrap();

        let search = bin.to_string_lossy().into_owned();
        let before = provider_release(Path::new("claude"), Some(&search)).unwrap();
        assert_eq!(before.version.as_deref(), Some("2.1.235"));

        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&newer, &link).unwrap();
        let after = provider_release(Path::new("claude"), Some(&search)).unwrap();

        assert_eq!(after.version.as_deref(), Some("2.1.236"));
        assert_ne!(before.resolved_path, after.resolved_path);
    }

    #[test]
    fn a_provider_with_no_version_in_its_path_is_still_told_apart() {
        // Nothing is invented for a layout that does not carry a version. The
        // resolved path still distinguishes one install from another.
        let root = tempfile::TempDir::new().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("codex"), b"binary").unwrap();

        let release = provider_release(Path::new("codex"), Some(&bin.to_string_lossy())).unwrap();

        assert_eq!(release.version, None);
        assert!(release.resolved_path.ends_with("codex"));
    }

    #[test]
    fn a_session_older_than_the_installed_release_is_superseded() {
        // A provider process runs the release it started with. This is the
        // whole question the runtime area needs answered: is this worker still
        // executing something that has since been replaced on disk.
        let release = ProviderRelease {
            resolved_path: "/versions/2.1.236".into(),
            version: Some("2.1.236".into()),
            installed_at: Some(1_000),
        };

        assert!(provider_release_superseded(Some(&release), 999));
        assert!(!provider_release_superseded(Some(&release), 1_000));
        assert!(!provider_release_superseded(Some(&release), 1_001));
    }

    #[test]
    fn an_unknown_release_never_asks_for_a_restart() {
        // Reporting a restart that is not needed teaches the operator to ignore
        // the one that is.
        assert!(!provider_release_superseded(None, 0));
        assert!(!provider_release_superseded(
            Some(&ProviderRelease {
                resolved_path: "/usr/bin/codex".into(),
                version: None,
                installed_at: None,
            }),
            0
        ));
    }

    #[test]
    fn a_provider_that_is_not_on_the_path_reports_nothing() {
        let root = tempfile::TempDir::new().unwrap();
        assert!(
            provider_release(Path::new("claude"), Some(&root.path().to_string_lossy())).is_none()
        );
        assert!(provider_release(Path::new("claude"), None).is_none());
    }

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
                &command.arguments[command.arguments.len() - 3..],
                [
                    "--mcp-config",
                    "/state/swarm-next/agents/worker.json",
                    "--strict-mcp-config"
                ]
            );
        }
    }

    #[test]
    fn claude_layers_operator_settings_without_replacing_private_session_state() {
        let command = ClaudeCodeAdapter
            .command_for_with_configuration(
                Path::new("/workspace/example"),
                ClaudeConversationStart::Continue,
                Some(Path::new("/state/swarm-next/agents/worker.json")),
                Some(Path::new("/home/operator/.claude/settings.json")),
            )
            .unwrap();
        assert_eq!(
            command.arguments,
            [
                "--continue",
                "--mcp-config",
                "/state/swarm-next/agents/worker.json",
                "--strict-mcp-config",
                "--settings",
                "/home/operator/.claude/settings.json",
            ]
        );
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
        let session_id = ProviderConversationId::new();
        let imported = adapter
            .command_for(
                Path::new("/workspaces/example"),
                CodexConversationStart::Resume { session_id },
            )
            .unwrap();
        assert_eq!(imported.arguments, ["resume", &session_id.to_string()]);
    }
}

/// What a provider executable resolves to on disk right now.
///
/// Claude and Codex update themselves. A running provider keeps executing the
/// release it was started with, so an update that lands while workers are up is
/// installed but not running, and stays that way until each worker restarts.
/// Recording what a session started against is what makes that difference
/// visible instead of silent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderRelease {
    /// The executable after following every symlink.
    pub resolved_path: String,
    /// The release name, when the resolved path carries one.
    ///
    /// Claude installs each release as `.../versions/<version>` and points a
    /// symlink at it, so the file name is the version. Nothing is invented when
    /// a layout does not work that way; the resolved path still distinguishes
    /// one release from another.
    pub version: Option<String>,
    /// When this release landed on disk, in unix seconds.
    ///
    /// A provider process runs the release it started with, so a session that
    /// began before this is running something older no matter which version
    /// that was. That comparison is what makes "installed but not running"
    /// answerable without recording a version per session.
    pub installed_at: Option<i64>,
}

/// Resolves a provider executable the way exec would, and reports what it found.
///
/// `search_path` is the `PATH` to search, so a caller can ask about an
/// environment other than its own — the terminal host spawns providers, and its
/// `PATH` is the one that decides which release a worker gets.
#[must_use]
pub fn provider_release(executable: &Path, search_path: Option<&str>) -> Option<ProviderRelease> {
    let candidate = if executable.components().count() > 1 {
        executable.to_path_buf()
    } else {
        search_path?
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(|entry| Path::new(entry).join(executable))
            .find(|candidate| candidate.is_file())?
    };
    let resolved = std::fs::canonicalize(candidate).ok()?;
    let version = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.starts_with(|c: char| c.is_ascii_digit()) && name.contains('.'))
        .map(ToOwned::to_owned);
    let installed_at = std::fs::metadata(&resolved)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_secs()).ok());
    Some(ProviderRelease {
        resolved_path: resolved.to_string_lossy().into_owned(),
        version,
        installed_at,
    })
}

/// Whether a session started before the release its provider now resolves to.
///
/// Conservative on purpose: with no release, no install time, or no session
/// start it answers no. Reporting a restart that is not needed teaches the
/// operator to ignore the one that is.
#[must_use]
pub fn provider_release_superseded(
    release: Option<&ProviderRelease>,
    session_started_at: i64,
) -> bool {
    release
        .and_then(|release| release.installed_at)
        .is_some_and(|installed_at| session_started_at < installed_at)
}
