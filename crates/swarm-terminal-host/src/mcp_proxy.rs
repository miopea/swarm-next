use std::{env, future::Future, time::Duration};

use reqwest::{Client, Response, header};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MCP_SESSION_HEADER: &str = "mcp-session-id";

#[derive(Clone, Copy)]
struct DiscoveryRecovery {
    attempts: usize,
    budget: Duration,
    first_backoff: Duration,
}

const DISCOVERY_RECOVERY: DiscoveryRecovery = DiscoveryRecovery {
    attempts: 6,
    budget: Duration::from_secs(20),
    first_backoff: Duration::from_millis(500),
};

#[derive(Debug, Error)]
pub(crate) enum McpProxyError {
    #[error("{0} is required")]
    MissingEnvironment(&'static str),
    #[error("MCP HTTP transport failed: {0}")]
    Client(#[from] reqwest::Error),
    #[error(
        "MCP discovery recovery deadline exhausted; reconnect the Swarm server from the provider's MCP controls"
    )]
    DiscoveryDeadline,
    #[error("MCP HTTP server returned status {0}")]
    HttpStatus(u16),
    #[error("MCP stdio failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("MCP message was not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub(crate) async fn run() -> Result<(), McpProxyError> {
    let url = required_env("SWARM_MCP_URL")?;
    let authorization = required_env("SWARM_MCP_AUTHORIZATION")?;
    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let mut session_id = None;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message = serde_json::from_str::<Value>(&line)?;
        let request_id = message.get("id").cloned();
        match recover_discovery(
            message.get("method").and_then(Value::as_str),
            || forward(&client, &url, &authorization, session_id.as_deref(), &line),
            DISCOVERY_RECOVERY,
        )
        .await
        {
            Ok(forwarded) => {
                if forwarded.session_id.is_some() {
                    session_id = forwarded.session_id;
                }
                for response in forwarded.messages {
                    write_json_line(&mut output, &response).await?;
                }
            }
            Err(error) => {
                if let Some(request_id) = request_id {
                    write_json_line(
                        &mut output,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "error": { "code": -32000, "message": error.to_string() }
                        }),
                    )
                    .await?;
                } else {
                    eprintln!("Swarm MCP notification failed: {error}");
                }
            }
        }
    }
    if let Some(session_id) = session_id.as_deref() {
        close_session(&client, &url, &authorization, session_id).await;
    }
    Ok(())
}

/// Only handshake/discovery can be repeated. An uncertain tool call may have
/// committed a side effect, even when the bridge received no response.
async fn recover_discovery<F, Fut>(
    method: Option<&str>,
    mut operation: F,
    policy: DiscoveryRecovery,
) -> Result<ForwardedResponse, McpProxyError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<ForwardedResponse, McpProxyError>>,
{
    if !matches!(method, Some("initialize" | "tools/list")) {
        return operation().await;
    }
    let deadline = tokio::time::Instant::now() + policy.budget;
    let mut backoff = policy.first_backoff;
    for attempt in 0..policy.attempts {
        let result = tokio::time::timeout_at(deadline, operation())
            .await
            .map_err(|_| McpProxyError::DiscoveryDeadline)?;
        let retryable = match &result {
            Err(McpProxyError::Client(error)) => error.is_connect() || error.is_timeout(),
            Err(McpProxyError::HttpStatus(502..=504)) => true,
            _ => false,
        };
        if !retryable || attempt + 1 == policy.attempts {
            return result;
        }
        tokio::time::timeout_at(deadline, tokio::time::sleep(backoff))
            .await
            .map_err(|_| McpProxyError::DiscoveryDeadline)?;
        backoff = backoff.saturating_mul(2);
    }
    Err(McpProxyError::DiscoveryDeadline)
}

struct ForwardedResponse {
    session_id: Option<String>,
    messages: Vec<Value>,
}

async fn forward(
    client: &Client,
    url: &str,
    authorization: &str,
    session_id: Option<&str>,
    body: &str,
) -> Result<ForwardedResponse, McpProxyError> {
    let mut request = client
        .post(url)
        .header(header::AUTHORIZATION, authorization)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .body(body.to_owned());
    if let Some(session_id) = session_id {
        request = request.header(MCP_SESSION_HEADER, session_id);
    }
    let response = request.send().await?;
    response_payload(response).await
}

async fn close_session(client: &Client, url: &str, authorization: &str, session_id: &str) {
    let _ = client
        .delete(url)
        .header(header::AUTHORIZATION, authorization)
        .header(MCP_SESSION_HEADER, session_id)
        .send()
        .await;
}

async fn response_payload(response: Response) -> Result<ForwardedResponse, McpProxyError> {
    let status = response.status();
    // The HTTP status owns error classification. Reading a broken error body
    // first can hide a retryable 503 (or a terminal 401) as a body transport
    // failure. Error bodies are neither needed nor safe to echo to providers.
    if !status.is_success() {
        return Err(McpProxyError::HttpStatus(status.as_u16()));
    }
    let session_id = response
        .headers()
        .get(MCP_SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.bytes().await?;
    if bytes.is_empty() {
        return Ok(ForwardedResponse {
            session_id,
            messages: Vec::new(),
        });
    }
    let text = String::from_utf8_lossy(&bytes);
    let messages = if content_type.starts_with("text/event-stream") {
        sse_messages(&text)?
    } else {
        vec![serde_json::from_str(text.trim())?]
    };
    Ok(ForwardedResponse {
        session_id,
        messages,
    })
}

fn sse_messages(body: &str) -> Result<Vec<Value>, serde_json::Error> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .filter(|data| !data.is_empty())
        .map(serde_json::from_str)
        .collect()
}

async fn write_json_line(
    output: &mut (impl AsyncWrite + Unpin),
    message: &Value,
) -> Result<(), McpProxyError> {
    output.write_all(&serde_json::to_vec(message)?).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}

fn required_env(name: &'static str) -> Result<String, McpProxyError> {
    env::var(name).map_err(|_| McpProxyError::MissingEnvironment(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RECOVERY: DiscoveryRecovery = DiscoveryRecovery {
        attempts: 3,
        budget: Duration::from_secs(1),
        first_backoff: Duration::ZERO,
    };

    #[tokio::test]
    async fn transient_discovery_failure_recovers_without_changing_the_response() {
        for method in ["initialize", "tools/list"] {
            let mut calls = 0;
            let result = recover_discovery(
                Some(method),
                || {
                    calls += 1;
                    std::future::ready(if calls == 1 {
                        Err(McpProxyError::HttpStatus(503))
                    } else {
                        Ok(ForwardedResponse {
                            session_id: Some("same-session".into()),
                            messages: vec![json!({"jsonrpc":"2.0","id":17,"result":{"tools":[]}})],
                        })
                    })
                },
                TEST_RECOVERY,
            )
            .await
            .unwrap();
            assert_eq!(calls, 2);
            assert_eq!(result.session_id.as_deref(), Some("same-session"));
            assert_eq!(result.messages[0]["id"], 17);
        }
    }

    #[tokio::test]
    async fn uncertain_writes_notifications_and_unknown_methods_are_never_replayed() {
        for method in [
            Some("tools/call"),
            Some("notifications/initialized"),
            Some("other"),
            None,
        ] {
            let mut calls = 0;
            let result = recover_discovery(
                method,
                || {
                    calls += 1;
                    std::future::ready(Err(McpProxyError::HttpStatus(503)))
                },
                TEST_RECOVERY,
            )
            .await;
            assert!(matches!(result, Err(McpProxyError::HttpStatus(503))));
            assert_eq!(calls, 1);
        }
    }

    #[tokio::test]
    async fn discovery_retries_are_bounded_and_do_not_retry_configuration_failures() {
        for (status, expected) in [
            (401, 1),
            (403, 1),
            (404, 1),
            (429, 1),
            (500, 1),
            (502, 3),
            (503, 3),
            (504, 3),
        ] {
            let mut calls = 0;
            let result = recover_discovery(
                Some("initialize"),
                || {
                    calls += 1;
                    std::future::ready(Err(McpProxyError::HttpStatus(status)))
                },
                TEST_RECOVERY,
            )
            .await;
            assert!(matches!(result, Err(McpProxyError::HttpStatus(value)) if value == status));
            assert_eq!(calls, expected);
        }
    }

    #[tokio::test]
    async fn deadline_cancels_a_hung_discovery_without_starting_another_attempt() {
        let mut calls = 0;
        let result = recover_discovery(
            Some("tools/list"),
            || {
                calls += 1;
                std::future::pending()
            },
            DiscoveryRecovery {
                budget: Duration::from_millis(10),
                ..TEST_RECOVERY
            },
        )
        .await;
        assert!(matches!(result, Err(McpProxyError::DiscoveryDeadline)));
        assert_eq!(calls, 1);
    }

    #[test]
    fn parses_only_sse_data_events_as_json_rpc_messages() {
        let messages =
            sse_messages("event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n")
                .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], 1);
    }
}
