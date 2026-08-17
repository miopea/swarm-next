use std::{env, time::Duration};

use reqwest::{Client, Response, header};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MCP_SESSION_HEADER: &str = "mcp-session-id";

#[derive(Debug, Error)]
pub(crate) enum McpProxyError {
    #[error("{0} is required")]
    MissingEnvironment(&'static str),
    #[error("failed to build the MCP HTTP client: {0}")]
    Client(#[from] reqwest::Error),
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
        match forward(&client, &url, &authorization, session_id.as_deref(), &line).await {
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
    if !status.is_success() {
        return Err(McpProxyError::HttpStatus(status.as_u16()));
    }
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

    #[test]
    fn parses_only_sse_data_events_as_json_rpc_messages() {
        let messages =
            sse_messages("event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n")
                .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], 1);
    }
}
