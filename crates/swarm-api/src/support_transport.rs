//! Bounded central-support HTTP transport. It owns no retry loop or Hive state.
use std::time::Duration;

use swarm_application::SupportDestination;
use swarm_persistence::{SupportReceipt, SupportRefusal};

const MAX_RECEIPT_BYTES: usize = 16 * 1024;
const MAX_SUBMISSION_BYTES: usize = 128 * 1024;
const DEADLINE: Duration = Duration::from_secs(20);

#[cfg(test)]
#[path = "support_paired_test.rs"]
mod paired_tests;

/// An HTTP receipt is still untrusted until the application fences its identities.
pub enum SupportTransportResult {
    Receipt(SupportReceipt),
    Refused {
        reason: swarm_persistence::SupportRefusal,
        retry_after_seconds: Option<u32>,
    },
    /// No accepted receipt: never infer rejection, generate a new key, or claim sent.
    Uncertain,
}

/// No cookies, authorization forwarding, redirects, or adapter-owned retries.
#[derive(Clone)]
pub struct SupportTransport {
    client: reqwest::Client,
    destination: SupportDestination,
}

impl SupportTransport {
    /// # Errors
    /// Refuses transport initialization failure before any report is claimed.
    pub fn new(destination: SupportDestination) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: transport_client(DEADLINE)?,
            destination,
        })
    }

    /// Sends the exact frozen payload to the deployment-owned endpoint.
    /// The process owner bounds concurrency, claims first, and durably settles afterward.
    /// Dropping this future cancels the request; an interrupted claim stays uncertain.
    pub async fn send(&self, frozen_submission: &str) -> SupportTransportResult {
        if frozen_submission.len() > MAX_SUBMISSION_BYTES {
            return SupportTransportResult::Uncertain;
        }
        match tokio::time::timeout(
            DEADLINE,
            receive(&self.client, self.destination.endpoint(), frozen_submission),
        )
        .await
        {
            Ok(Some(result)) => result,
            _ => SupportTransportResult::Uncertain,
        }
    }
}

fn transport_client(deadline: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(deadline)
        .build()
}

async fn receive(
    client: &reqwest::Client,
    endpoint: &str,
    frozen_submission: &str,
) -> Option<SupportTransportResult> {
    let mut response = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(frozen_submission.to_owned())
        .send()
        .await
        .ok()?;
    let refusal = match response.status().as_u16() {
        409 => Some((SupportRefusal::Conflict, None)),
        429 => Some((
            SupportRefusal::RateLimited,
            response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(retry_seconds),
        )),
        400 | 401 | 403 | 404 | 413 | 422 => Some((SupportRefusal::Rejected, None)),
        _ => None,
    };
    if let Some((reason, retry_after_seconds)) = refusal {
        return Some(SupportTransportResult::Refused {
            reason,
            retry_after_seconds,
        });
    }
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_RECEIPT_BYTES as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > MAX_RECEIPT_BYTES.saturating_sub(bytes.len()) {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .ok()
        .map(SupportTransportResult::Receipt)
}

fn retry_seconds(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|seconds| (1..=604_800).contains(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Response, routing::post};

    async fn server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (endpoint, task)
    }

    fn client() -> reqwest::Client {
        transport_client(Duration::from_secs(2)).unwrap()
    }

    #[test]
    fn retry_after_is_bounded_seconds_not_arbitrary_header_text() {
        assert_eq!(retry_seconds("120"), Some(120));
        for invalid in [
            "",
            "0",
            "-1",
            "604801",
            "1234567",
            "1.5",
            " 60",
            "Wed, 09 Sep 2026 00:00:00 GMT",
        ] {
            assert_eq!(retry_seconds(invalid), None);
        }
    }

    #[tokio::test]
    async fn conflict_and_rate_limit_are_typed_without_reading_error_bodies() {
        for status in [409, 429] {
            let router = Router::new().route(
                "/",
                post(move || async move {
                    Response::builder()
                        .status(status)
                        .header("retry-after", "120")
                        .body(Body::from_stream(futures_util::stream::pending::<
                            Result<Vec<u8>, std::io::Error>,
                        >()))
                        .unwrap()
                }),
            );
            let (endpoint, task) = server(router).await;
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                receive(&client(), &endpoint, "fictional"),
            )
            .await
            .unwrap()
            .unwrap();
            let SupportTransportResult::Refused {
                reason,
                retry_after_seconds,
            } = result
            else {
                panic!("expected refusal")
            };
            assert!(matches!(
                (status, reason),
                (409, swarm_persistence::SupportRefusal::Conflict)
                    | (429, swarm_persistence::SupportRefusal::RateLimited)
            ));
            assert_eq!(
                retry_after_seconds,
                if status == 429 { Some(120) } else { None }
            );
            task.abort();
            let _ = task.await;
        }
    }

    #[tokio::test]
    async fn sends_exact_frozen_bytes_and_reads_receipt_without_claiming_confirmation() {
        let router = Router::new().route(
            "/",
            post(|body: String| async move {
                assert_eq!(body, "{\"reviewed\":\"fictional only\"}");
                axum::Json(SupportReceipt {
                    submission_key: "untrusted-key".into(),
                    conversation_id: "untrusted-thread".into(),
                    message_id: "untrusted-message".into(),
                    created_at: 1,
                    deduplicated: false,
                })
            }),
        );
        let (endpoint, task) = server(router).await;
        let result = receive(&client(), &endpoint, "{\"reviewed\":\"fictional only\"}")
            .await
            .unwrap();
        let SupportTransportResult::Receipt(receipt) = result else {
            panic!("expected a receipt")
        };
        assert_eq!(receipt.submission_key, "untrusted-key");
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn redirect_is_not_followed_and_error_bodies_are_not_receipts() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let hits = Arc::new(AtomicUsize::new(0));
        let observed = hits.clone();
        let router = Router::new()
            .route(
                "/",
                post(|| async {
                    Response::builder()
                        .status(307)
                        .header("location", "/private")
                        .body(Body::empty())
                        .unwrap()
                }),
            )
            .route(
                "/private",
                post(move || {
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        "must not reach here"
                    }
                }),
            );
        let (endpoint, task) = server(router).await;
        assert!(receive(&client(), &endpoint, "fictional").await.is_none());
        assert_eq!(observed.load(Ordering::SeqCst), 0);
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn oversized_chunked_and_invalid_receipts_are_uncertain() {
        let router = Router::new().route(
            "/",
            post(|| async {
                // No Content-Length shortcut: the byte limit applies while streaming.
                let chunks = futures_util::stream::iter([
                    Ok::<_, std::io::Error>(vec![b' '; MAX_RECEIPT_BYTES]),
                    Ok(br#"{"submission_key":"key","conversation_id":"thread","message_id":"message","created_at":1,"deduplicated":false}"#.to_vec()),
                ]);
                Body::from_stream(chunks)
            }),
        );
        let (endpoint, task) = server(router).await;
        assert!(receive(&client(), &endpoint, "fictional").await.is_none());
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn interrupted_body_cannot_hold_the_sender_beyond_its_deadline() {
        let router = Router::new().route(
            "/",
            post(|| async {
                Body::from_stream(futures_util::stream::pending::<
                    Result<Vec<u8>, std::io::Error>,
                >())
            }),
        );
        let (endpoint, task) = server(router).await;
        let client = transport_client(Duration::from_millis(100)).unwrap();
        // The outer bound makes a missing client timeout fail instead of hanging the suite.
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            receive(&client, &endpoint, "fictional"),
        )
        .await
        .expect("transport must terminate an unfinished response");
        assert!(result.is_none());
        task.abort();
        let _ = task.await;
    }
}
