use std::{ffi::OsString, io, path::Path, process::Stdio, time::Duration};

use serde::Deserialize;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

pub(super) const MAX_CONTEXT_BYTES: usize = 12 * 1024;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_ERROR_BYTES: usize = 4 * 1024;
const IMPROVEMENT_TIMEOUT: Duration = Duration::from_secs(45);
const DESCRIPTION_SCHEMA: &str = r#"{"type":"object","properties":{"description":{"type":"string","maxLength":2000}},"required":["description"],"additionalProperties":false}"#;

#[derive(Debug, Error)]
pub(super) enum DescriptionAiError {
    #[error("Claude Code is not available for this bounded description review")]
    Unavailable,
    #[error("Claude Code did not finish the description review within 45 seconds")]
    TimedOut,
    #[error("Claude Code did not return a valid routing description")]
    InvalidResponse,
    #[error("Claude Code could not complete the description review")]
    Failed,
}

#[derive(Debug, Deserialize)]
struct ClaudeEnvelope {
    structured_output: Option<DescriptionResult>,
}

#[derive(Debug, Deserialize)]
struct DescriptionResult {
    description: String,
}

pub(super) async fn improve_description(context: &str) -> Result<String, DescriptionAiError> {
    if context.is_empty() || context.len() > MAX_CONTEXT_BYTES {
        return Err(DescriptionAiError::InvalidResponse);
    }
    let scratch = tempfile::tempdir().map_err(|_| DescriptionAiError::Failed)?;
    let mcp_config = scratch.path().join("mcp.json");
    tokio::fs::write(&mcp_config, r#"{"mcpServers":{}}"#)
        .await
        .map_err(|_| DescriptionAiError::Failed)?;
    let executable =
        std::env::var_os("SWARM_CLAUDE_COMMAND").unwrap_or_else(|| OsString::from("claude"));
    run_claude(executable, scratch.path(), &mcp_config, context).await
}

async fn run_claude(
    executable: OsString,
    scratch: &Path,
    mcp_config: &Path,
    context: &str,
) -> Result<String, DescriptionAiError> {
    let mut command = Command::new(executable);
    command
        .args(claude_arguments(mcp_config))
        .current_dir(scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| DescriptionAiError::Unavailable)?;
    let mut stdin = child.stdin.take().ok_or(DescriptionAiError::Failed)?;
    stdin
        .write_all(description_prompt(context).as_bytes())
        .await
        .map_err(|_| DescriptionAiError::Failed)?;
    drop(stdin);
    let stdout = child.stdout.take().ok_or(DescriptionAiError::Failed)?;
    let stderr = child.stderr.take().ok_or(DescriptionAiError::Failed)?;
    let completed = timeout(IMPROVEMENT_TIMEOUT, async {
        tokio::join!(
            child.wait(),
            read_bounded(stdout, MAX_OUTPUT_BYTES),
            read_bounded(stderr, MAX_ERROR_BYTES)
        )
    })
    .await;
    let Ok((status, stdout, _stderr)) = completed else {
        let _ = child.kill().await;
        return Err(DescriptionAiError::TimedOut);
    };
    if !status.map_err(|_| DescriptionAiError::Failed)?.success() {
        return Err(DescriptionAiError::Failed);
    }
    let (stdout, overflowed) = stdout.map_err(|_| DescriptionAiError::Failed)?;
    if overflowed {
        return Err(DescriptionAiError::InvalidResponse);
    }
    parse_description(&stdout)
}

fn claude_arguments(mcp_config: &Path) -> Vec<OsString> {
    vec![
        "--print".into(),
        "--tools".into(),
        "".into(),
        "--disallowedTools".into(),
        "mcp__*".into(),
        "--no-session-persistence".into(),
        "--max-turns".into(),
        "1".into(),
        "--max-budget-usd".into(),
        "0.10".into(),
        "--output-format".into(),
        "json".into(),
        "--json-schema".into(),
        DESCRIPTION_SCHEMA.into(),
        "--setting-sources".into(),
        "project".into(),
        "--mcp-config".into(),
        mcp_config.as_os_str().to_owned(),
        "--strict-mcp-config".into(),
    ]
}

fn description_prompt(context: &str) -> String {
    format!(
        "Write one concise routing description for a software worker. Explain what this repository owns and when Queen should route work to it. Use only the bounded metadata below; do not infer confidential data or claim capabilities not shown. Return only the required structured description.\n\n{context}"
    )
}

pub(super) async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> io::Result<(Vec<u8>, bool)> {
    let mut stored = Vec::with_capacity(limit.min(8 * 1024));
    let mut overflowed = false;
    let mut chunk = [0_u8; 4096];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok((stored, overflowed));
        }
        let remaining = limit.saturating_sub(stored.len());
        stored.extend_from_slice(&chunk[..count.min(remaining)]);
        overflowed |= count > remaining;
    }
}

fn parse_description(bytes: &[u8]) -> Result<String, DescriptionAiError> {
    let envelope = serde_json::from_slice::<ClaudeEnvelope>(bytes)
        .map_err(|_| DescriptionAiError::InvalidResponse)?;
    let description = envelope
        .structured_output
        .ok_or(DescriptionAiError::InvalidResponse)?
        .description;
    super::workers::clean_summary(&description).ok_or(DescriptionAiError::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_is_one_bounded_tool_free_non_persistent_turn() {
        let arguments = claude_arguments(Path::new("/private/empty-mcp.json"));
        let arguments = arguments
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(arguments.windows(2).any(|pair| pair == ["--tools", ""]));
        assert!(arguments.contains(&"--no-session-persistence".into()));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--max-turns", "1"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--max-budget-usd", "0.10"])
        );
        assert!(arguments.contains(&"--strict-mcp-config".into()));
    }

    #[test]
    fn parser_accepts_only_the_structured_bounded_description() {
        let parsed = parse_description(
            br#"{"result":"ignored","structured_output":{"description":"Meadow owns garden planning.\nRoute related work here."}}"#,
        )
        .unwrap();
        assert_eq!(
            parsed,
            "Meadow owns garden planning. Route related work here."
        );
        assert!(parse_description(br#"{"result":"unstructured prose"}"#).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_runner_accepts_a_fake_structured_cli_without_using_real_claude() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = tempfile::tempdir().unwrap();
        let executable = scratch.path().join("fake-claude");
        tokio::fs::write(
            &executable,
            "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"structured_output\":{\"description\":\"Clover owns garden coordination.\"}}'\n",
        )
        .await
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mcp_config = scratch.path().join("mcp.json");
        tokio::fs::write(&mcp_config, r#"{"mcpServers":{}}"#)
            .await
            .unwrap();

        let result = run_claude(
            executable.into_os_string(),
            scratch.path(),
            &mcp_config,
            "Repository name: Clover",
        )
        .await
        .unwrap();
        assert_eq!(result, "Clover owns garden coordination.");
    }
}
