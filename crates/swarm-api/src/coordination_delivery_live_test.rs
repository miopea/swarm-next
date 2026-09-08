//! Explicit opt-in acceptance on the existing fictional workflow worker.
//! No fault switch is compiled into the application. Only this test's private
//! proxy withholds one Enter; other clients use the real host unchanged.

use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const WORKER: &str = "01a06eda-bdd1-7a82-928e-cffbee0be6c1";
const WORKSPACE: &str = "/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture";

async fn metadata(path: &str, token: &str) -> serde_json::Value {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut response = client
        .get(format!("http://127.0.0.1:8766/api/v1/{path}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        assert!(
            body.len() + chunk.len() <= 4 * 1024 * 1024,
            "metadata exceeded test budget"
        );
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).unwrap()
}

async fn unused_demo(token: &str, expected: Option<WorkerSessionId>) -> WorkerSessionId {
    let profiles = metadata("workers", token).await;
    let profile = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["id"] == WORKER)
        .expect("explicit demo worker missing");
    assert_eq!(profile["name"], "Swarm Dogfood");
    assert_eq!(profile["workspace"], WORKSPACE);
    assert_eq!(profile["provider"], "claude_code");
    assert_eq!(profile["provider_activity"], "resting", "demo is not idle");
    assert_eq!(profile["background_work"], false);
    assert_eq!(
        profile["attention_state"], "resting",
        "demo must not be operator-engaged"
    );
    assert!(
        profile
            .get("engaged_device")
            .is_none_or(serde_json::Value::is_null),
        "demo has an engaged device"
    );
    let session: WorkerSessionId = profile["active_session_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    if let Some(expected) = expected {
        assert_eq!(session, expected, "demo session changed");
    }
    let tasks = metadata("tasks", token).await;
    assert!(
        !tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["assigned_worker_id"] == WORKER),
        "demo has open work; leave it untouched"
    );
    session
}

async fn require_empty_demo(host: &HostClient, session: WorkerSessionId, marker: &[u8]) {
    assert!(
        matches!(
            delivery_baseline(host, session, ProviderKind::ClaudeCode, marker)
                .await
                .unwrap(),
            Baseline::Ready { .. }
        ),
        "demo prompt is not positively empty; no input written"
    );
}

#[tokio::test]
#[ignore = "writes one fictional prompt; requires explicit live-demo opt-in and local operator token"]
async fn live_demo_recovers_one_withheld_enter_without_repasting() {
    assert_eq!(
        std::env::var("SWARM_DOGFOOD_UNSENT_ACCEPT").as_deref(),
        Ok("workflow-fixture-only")
    );
    let token =
        std::env::var("SWARM_OPERATOR_TOKEN").expect("operator token must be supplied privately");
    let socket =
        std::env::var("SWARM_TERMINAL_SOCKET").expect("exact running engine socket required");
    let session = unused_demo(&token, None).await;
    let marker = format!("[Swarm dogfood recovery {}]", WorkerSessionId::new());
    let prompt = format!("{marker} Fictional transport acceptance only. Reply exactly RECOVERY RECEIVED. Do not use tools, change tasks or files, or contact anyone.\r").into_bytes();
    let host = HostClient::new(socket);
    require_empty_demo(&host, session, marker.as_bytes()).await;

    let directory = tempfile::tempdir().unwrap();
    let proxy_path = directory.path().join("recovery.sock");
    let listener = tokio::net::UnixListener::bind(&proxy_path).unwrap();
    let payloads = Arc::new(AtomicUsize::new(0));
    let enters = Arc::new(AtomicUsize::new(0));
    let proxy_payloads = payloads.clone();
    let proxy_enters = enters.clone();
    let proxy_token = token.clone();
    let allowed_payload = prompt[..prompt.len() - 1].to_vec();
    let server = tokio::spawn(async move {
        for _ in 0..800 {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream).take(256 * 1024 + 1);
            let mut line = Vec::new();
            reader.read_until(b'\n', &mut line).await.unwrap();
            assert!(line.len() <= 256 * 1024, "oversized test request");
            let request: HostRequest = serde_json::from_slice(&line).unwrap();
            let response = match &request {
                HostRequest::Read { session_id, .. } if *session_id == session => {
                    coordination_request(&host, &request)
                        .await
                        .unwrap()
                        .expect("host read timed out")
                }
                HostRequest::Write {
                    session_id, bytes, ..
                } if *session_id == session => {
                    unused_demo(&proxy_token, Some(session)).await;
                    if bytes == b"\r" && proxy_enters.fetch_add(1, Ordering::SeqCst) == 0 {
                        HostResponse::Error {
                            code: "dogfood_enter_withheld".into(),
                            message: "Only this test's first Enter was not forwarded".into(),
                        }
                    } else {
                        if bytes != b"\r" {
                            assert_eq!(
                                bytes, &allowed_payload,
                                "only the fictional payload is allowed"
                            );
                            assert_eq!(
                                proxy_payloads.fetch_add(1, Ordering::SeqCst),
                                0,
                                "refuse duplicate paste"
                            );
                        }
                        coordination_request(&host, &request)
                            .await
                            .unwrap()
                            .expect("host write timed out")
                    }
                }
                _ => panic!("test proxy refuses any other session or operation"),
            };
            let mut encoded = serde_json::to_vec(&response).unwrap();
            encoded.push(b'\n');
            reader
                .get_mut()
                .get_mut()
                .write_all(&encoded)
                .await
                .unwrap();
        }
        panic!("test proxy request budget exhausted");
    });
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let proxy = HostClient::new(proxy_path);
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let first = submit_terminal_message(&proxy, session, ProviderKind::ClaudeCode, prompt.clone(), marker.as_bytes()).await.unwrap();
        assert_eq!(first, TerminalSubmission::Uncertain, "fault must create uncertainty, not a normal success");
        assert_eq!(payloads.load(Ordering::SeqCst), 1);
        assert_eq!(enters.load(Ordering::SeqCst), 1);
        assert!(matches!(delivery_baseline(&proxy, session, ProviderKind::ClaudeCode, marker.as_bytes()).await.unwrap(), Baseline::HoldsOurUnsentMessage { .. }),
            "backstop was not triggered: preserve the demo prompt for inspection");
        let recovered = submit_terminal_message(&proxy, session, ProviderKind::ClaudeCode, prompt, marker.as_bytes()).await.unwrap();
        assert_eq!(recovered, TerminalSubmission::Acknowledged);
        assert_eq!(payloads.load(Ordering::SeqCst), 1, "recovery must not paste again");
        assert_eq!(enters.load(Ordering::SeqCst), 2, "one withheld and one forwarded Enter");
        println!("LIVE DEMO: session={session}; first=uncertain; unsent_marker=observed; recovery=acknowledged; payload_writes=1; forwarded_enters=1");
    }).await;
    server.abort();
    let _ = server.await;
    result.expect("bounded demo acceptance timed out; do not replay input blindly");
}
