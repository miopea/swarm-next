//! Asks Swarm whether the operator granted this exact command (ADR 0129).
//!
//! Run by the grant mod's `tool.check` hook as
//! `swarm-terminal-host provider-grant <worker MCP config>` with
//! `{"command": "...", "cwd": "..."}` on standard input, `cwd` being the
//! directory the worker's shell is in. It prints `granted` only when Swarm
//! spent a live grant for exactly that command there, and `not granted`
//! otherwise.
//!
//! Every failure reads as `not granted`, so the worker's permission checks
//! decide exactly as they would have without a grant.
use std::{path::Path, time::Duration};

use reqwest::{Client, header};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

/// Larger than any command a request may carry, with room for JSON escaping.
const MAX_INPUT_BYTES: u64 = 64 * 1024;
/// A worker's MCP config is a few hundred bytes.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
/// Every shell command waits on this answer, so a stalled API costs at most
/// this much before the worker's own checks decide.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn run(config: Option<&str>) {
    let granted = match config {
        Some(config) => match read_input().await {
            Ok(input) => ask(Path::new(config), &input).await,
            Err(error) => Err(error),
        },
        None => Err("usage: swarm-terminal-host provider-grant <worker MCP config>".into()),
    };
    match granted {
        Ok(true) => println!("granted"),
        Ok(false) => println!("not granted"),
        Err(error) => {
            eprintln!("swarm-terminal-host provider-grant: {error}");
            println!("not granted");
        }
    }
}

async fn read_input() -> Result<Vec<u8>, String> {
    let mut input = Vec::new();
    tokio::io::stdin()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut input)
        .await
        .map_err(|error| format!("could not read the command: {error}"))?;
    Ok(input)
}

async fn ask(config: &Path, input: &[u8]) -> Result<bool, String> {
    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err("the command is longer than any grant can carry".into());
    }
    let input = serde_json::from_slice::<Value>(input).map_err(|_| "the input is not JSON")?;
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .ok_or("the input names no command")?;
    let cwd = input.get("cwd").and_then(Value::as_str);
    let mut config_bytes = Vec::new();
    tokio::fs::File::open(config)
        .await
        .map_err(|error| format!("could not open the worker's MCP config: {error}"))?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut config_bytes)
        .await
        .map_err(|error| format!("could not read the worker's MCP config: {error}"))?;
    let (url, authorization) = endpoint(&config_bytes)?;
    let client = Client::builder()
        .timeout(ANSWER_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(url)
        .header(header::AUTHORIZATION, authorization)
        .json(&json!({ "command": command, "cwd": cwd }))
        .send()
        .await
        .map_err(|error| format!("Swarm did not answer: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Swarm answered {}", response.status()));
    }
    let answer = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Swarm's answer was not JSON: {error}"))?;
    Ok(answer.get("granted").and_then(Value::as_bool) == Some(true))
}

/// The grant endpoint and the worker's own credential, from its MCP config.
///
/// The config names Swarm's server under whichever name it was written with,
/// so the server is found by the two variables the MCP proxy itself reads.
fn endpoint(config: &[u8]) -> Result<(String, String), String> {
    if config.len() as u64 > MAX_CONFIG_BYTES {
        return Err("the worker's MCP config is too large".into());
    }
    let config: Value =
        serde_json::from_slice(config).map_err(|_| "the worker's MCP config is not JSON")?;
    let servers = config
        .get("mcpServers")
        .and_then(Value::as_object)
        .ok_or("the worker's MCP config names no servers")?;
    servers
        .values()
        .find_map(|server| {
            let env = server.get("env")?;
            let url = env.get("SWARM_MCP_URL")?.as_str()?;
            let authorization = env.get("SWARM_MCP_AUTHORIZATION")?.as_str()?;
            Some((
                format!(
                    "{}{}",
                    url.trim_end_matches('/'),
                    swarm_terminal::COMMAND_GRANT_PATH_SUFFIX
                ),
                authorization.to_owned(),
            ))
        })
        .ok_or_else(|| "the worker's MCP config does not name Swarm".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_the_worker_mcp_url_with_the_grant_path() {
        let config = json!({
            "mcpServers": {
                "other": { "command": "x" },
                "swarm": {
                    "command": "swarm-terminal-host",
                    "env": {
                        "SWARM_MCP_URL": "http://127.0.0.1:8766/mcp",
                        "SWARM_MCP_AUTHORIZATION": "Bearer worker-token"
                    }
                }
            }
        });
        assert_eq!(
            endpoint(config.to_string().as_bytes()).unwrap(),
            (
                "http://127.0.0.1:8766/mcp/command-grant".to_owned(),
                "Bearer worker-token".to_owned()
            )
        );
        assert!(endpoint(b"{}").is_err());
        assert!(endpoint(b"not json").is_err());
    }

    /// Serves one canned HTTP answer and hands back the request it read.
    async fn one_answer(
        status: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let served = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(split) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            let answer = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(answer.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, served)
    }

    fn config_for(directory: &tempfile::TempDir, url: &str) -> std::path::PathBuf {
        let path = directory.path().join("worker.json");
        std::fs::write(
            &path,
            json!({ "mcpServers": { "swarm": { "env": {
                "SWARM_MCP_URL": url,
                "SWARM_MCP_AUTHORIZATION": "Bearer worker-token"
            } } } })
            .to_string(),
        )
        .unwrap();
        path
    }

    #[tokio::test]
    async fn granted_only_when_swarm_says_it_spent_a_grant_for_this_command() {
        let directory = tempfile::tempdir().unwrap();
        let input =
            json!({ "command": "cd /srv && ./release --slot 'a*'", "cwd": "/home/w" }).to_string();

        let (url, served) = one_answer("200 OK", r#"{"granted":true}"#).await;
        let config = config_for(&directory, &url);
        assert_eq!(ask(&config, input.as_bytes()).await, Ok(true));
        let request = served.await.unwrap();
        assert!(request.starts_with("POST /mcp/command-grant "), "{request}");
        assert!(
            request.contains("authorization: Bearer worker-token"),
            "{request}"
        );
        assert!(
            request.ends_with(r#"{"command":"cd /srv && ./release --slot 'a*'","cwd":"/home/w"}"#),
            "the exact text, and where it runs, are what Swarm compares: {request}"
        );

        let (url, served) = one_answer("200 OK", r#"{"granted":false}"#).await;
        let config = config_for(&directory, &url);
        assert_eq!(ask(&config, input.as_bytes()).await, Ok(false));
        served.await.unwrap();

        let (url, served) = one_answer("401 Unauthorized", "").await;
        let config = config_for(&directory, &url);
        assert!(ask(&config, input.as_bytes()).await.is_err());
        served.await.unwrap();
    }

    #[tokio::test]
    async fn unreadable_input_or_an_absent_swarm_grants_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let config = config_for(&directory, "http://127.0.0.1:9/mcp");
        let input = json!({ "command": "true" }).to_string();
        assert!(ask(&config, input.as_bytes()).await.is_err());
        assert!(ask(&config, b"not json").await.is_err());
        let oversized = vec![b' '; usize::try_from(MAX_INPUT_BYTES).unwrap() + 1];
        assert!(ask(&config, &oversized).await.is_err());
    }
}
