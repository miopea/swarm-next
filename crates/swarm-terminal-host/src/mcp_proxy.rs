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
    let mut surface = ToolSurface::default();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message = serde_json::from_str::<Value>(&line)?;
        let request_id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        match recover_discovery(
            method,
            || forward(&client, &url, &authorization, session_id.as_deref(), &line),
            DISCOVERY_RECOVERY,
        )
        .await
        {
            Ok(mut forwarded) => {
                if forwarded.session_id.is_some() {
                    session_id = forwarded.session_id.take();
                }
                if method == Some("initialize") {
                    forwarded
                        .messages
                        .iter_mut()
                        .for_each(announce_tool_changes);
                }
                let changed = surface.observe(method, forwarded.surface.as_deref());
                for response in forwarded.messages {
                    write_json_line(&mut output, &response).await?;
                }
                if changed {
                    write_json_line(
                        &mut output,
                        &json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}),
                    )
                    .await?;
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
    /// The revision of the tools the API serves, from its response header.
    surface: Option<String>,
    messages: Vec<Value>,
}

/// Which tools this connection last listed, and whether a newer set has
/// already been announced (ADR 0123).
///
/// An API reload can add a tool while a worker runs, and Claude asks for its
/// tools once. Claude refetches them when its server says they changed, so the
/// relay says so the first time a response carries a revision other than the
/// one it last listed. That is the next tool call after a reload: a worker
/// that just shipped a tool and asks whether the reload landed can then see it.
#[derive(Default)]
struct ToolSurface {
    listed: Option<String>,
    announced: Option<String>,
}

impl ToolSurface {
    /// Returns whether to tell Claude its tools changed.
    fn observe(&mut self, method: Option<&str>, seen: Option<&str>) -> bool {
        let Some(seen) = seen else {
            // An API that predates the header says nothing about its tools.
            return false;
        };
        if method == Some("tools/list") {
            self.listed = Some(seen.to_owned());
            self.announced = None;
            return false;
        }
        let moved = self.listed.as_deref().is_some_and(|listed| listed != seen)
            && self.announced.as_deref() != Some(seen);
        if moved {
            self.announced = Some(seen.to_owned());
        }
        moved
    }
}

/// Declares that this server's tools can change, so Claude acts on the notice.
fn announce_tool_changes(response: &mut Value) {
    if let Some(tools) = response
        .pointer_mut("/result/capabilities/tools")
        .and_then(Value::as_object_mut)
    {
        tools.insert("listChanged".to_owned(), Value::Bool(true));
    }
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
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let session_id = header(MCP_SESSION_HEADER);
    let surface = header(swarm_terminal::AGENT_TOOL_SURFACE_HEADER);
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
            surface,
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
        surface,
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
                            surface: None,
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

    /// Measured against Claude Code 2.1.310 on 2026-10-23: after this notice
    /// it lists the server's tools again at once, while idle.
    #[test]
    fn a_new_tool_revision_is_announced_once_until_the_tools_are_listed_again() {
        let mut surface = ToolSurface::default();
        // Nothing to compare against before the first listing.
        assert!(!surface.observe(Some("tools/call"), Some("34")));
        assert!(!surface.observe(Some("tools/list"), Some("34")));
        assert!(!surface.observe(Some("tools/call"), Some("34")));
        // An API reload serves revision 35.
        assert!(surface.observe(Some("tools/call"), Some("35")));
        assert!(
            !surface.observe(Some("tools/call"), Some("35")),
            "announced once"
        );
        assert!(!surface.observe(Some("tools/list"), Some("35")));
        assert!(!surface.observe(Some("tools/call"), Some("35")));
        // An older API sends no header and is never announced.
        assert!(!surface.observe(Some("tools/call"), None));
    }

    #[test]
    fn the_initialize_answer_declares_that_tools_can_change() {
        let mut response = json!({"jsonrpc":"2.0","id":0,"result":{
            "protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"swarm"}}});
        announce_tool_changes(&mut response);
        assert_eq!(
            response["result"]["capabilities"]["tools"]["listChanged"],
            true
        );
        // A server offering no tools is not given a tools capability.
        let mut no_tools = json!({"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}});
        announce_tool_changes(&mut no_tools);
        assert_eq!(no_tools["result"]["capabilities"], json!({}));
        let mut error = json!({"jsonrpc":"2.0","id":0,"error":{"code":-32000,"message":"x"}});
        let before = error.clone();
        announce_tool_changes(&mut error);
        assert_eq!(error, before);
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
