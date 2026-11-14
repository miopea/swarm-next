use std::path::Path;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use swarm_terminal::HostClient;
use tower::ServiceExt as _;

use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfictional-screenshot";
const JPEG: &[u8] = b"\xff\xd8\xfffictional-photo";

fn sent(media_type: &str, bytes: &[u8]) -> SentImage {
    SentImage {
        media_type: media_type.into(),
        data: Base64::encode_string(bytes),
    }
}

fn files_in(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn an_image_is_written_privately_under_its_own_id_and_read_back() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("decision-images");
    let files = DecisionImageStore::new(root.clone());
    let store = TaskStore::in_memory().unwrap();

    let written = files
        .write(&store, &[sent("image/png", PNG), sent("IMAGE/JPEG", JPEG)])
        .await
        .unwrap();

    assert_eq!(written.len(), 2);
    assert_eq!(written[1].media_type, "image/jpeg");
    assert_eq!(
        files_in(&root),
        {
            let mut expected = vec![
                format!("{}.png", written[0].id),
                format!("{}.jpg", written[1].id),
            ];
            expected.sort();
            expected
        },
        "named by a fresh id, never by what they contain"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&root.join(format!("{}.png", written[0].id))), 0o600);
    }
    let recorded = swarm_persistence::DecisionImage {
        id: written[0].id,
        decision_id: swarm_domain::DecisionRequestId::new(),
        clarification_id: None,
        media_type: "image/png".into(),
        byte_len: PNG.len() as u64,
        attached_at: 1,
    };
    assert_eq!(files.read(&recorded).await.unwrap(), PNG);
    let mislabelled = swarm_persistence::DecisionImage {
        media_type: "image/jpeg".into(),
        ..recorded
    };
    assert_eq!(
        files.read(&mislabelled).await,
        Err(DecisionImageError::NotFound)
    );
}

#[tokio::test]
async fn a_refused_image_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("decision-images");
    let files = DecisionImageStore::new(root.clone());
    let store = TaskStore::in_memory().unwrap();
    let oversized = vec![0_u8; usize::try_from(MAX_DECISION_IMAGE_BYTES).unwrap() + 1];
    let mut oversized_png = PNG.to_vec();
    oversized_png.extend_from_slice(&oversized);

    for (images, expected) in [
        (vec![sent("image/png", PNG); 5], DecisionImageError::TooMany),
        (vec![sent("image/png", JPEG)], DecisionImageError::Invalid),
        (
            vec![sent("image/svg+xml", b"<svg/>")],
            DecisionImageError::Invalid,
        ),
        (
            vec![sent("image/png", &oversized_png)],
            DecisionImageError::Invalid,
        ),
        (
            vec![
                sent("image/png", PNG),
                SentImage {
                    media_type: "image/png".into(),
                    data: "not base64!".into(),
                },
            ],
            DecisionImageError::Invalid,
        ),
    ] {
        assert_eq!(
            files.write(&store, &images).await.err(),
            Some(expected),
            "{images:?}"
        );
    }
    assert!(files_in(&root).is_empty());
}

#[tokio::test]
async fn files_nothing_records_are_removed_once_old_and_recorded_ones_stay() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let recorded = uuid::Uuid::now_v7();
    let orphan = uuid::Uuid::now_v7();
    for id in [recorded, orphan] {
        std::fs::write(root.join(format!("{id}.png")), PNG).unwrap();
    }
    std::fs::write(root.join("stray.txt"), b"x").unwrap();
    let kept = HashSet::from([recorded]);

    remove_unrecorded(&root, &kept, Duration::from_secs(3600))
        .await
        .unwrap();
    assert_eq!(
        files_in(&root).len(),
        3,
        "too young to be sure nothing is recording it"
    );

    remove_unrecorded(&root, &kept, Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(files_in(&root), vec![format!("{recorded}.png")]);
}

struct Hive {
    app: Router,
    store: TaskStore,
    root: PathBuf,
    decision: swarm_domain::DecisionRequestId,
    _directory: tempfile::TempDir,
}

fn hive() -> Hive {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("decision-images");
    let store = TaskStore::in_memory().unwrap();
    let queen = store.ensure_queen("/fictional/queen").unwrap();
    let decision = store
        .create_decision_request(&swarm_persistence::NewDecisionRequest {
            requesting_worker_id: queen.id,
            task_id: None,
            kind: swarm_domain::DecisionRequestKind::Input,
            urgency: swarm_domain::DecisionUrgency::Normal,
            title: "Which fictional layout?",
            summary: "Choose the layout.",
            reason: "Contract test",
            risk: "No real work",
            evidence: "Fixture only",
            suggested_action: "Keep it",
            allowed_actions: &["Keep it".into(), "Change it".into()],
            operator_actions: &[],
            questions: &[],
            deadline: None,
            requested_command: None,
            requested_script: None,
        })
        .unwrap()
        .id;
    let app = crate::router(
        AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store.clone())
            .with_decision_image_store(root.clone()),
    );
    Hive {
        app,
        store,
        root,
        decision,
        _directory: directory,
    }
}

async fn send(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Vec<u8>) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer secret");
    let body = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&body).unwrap())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn an_answer_with_a_screenshot_reaches_the_inbox_and_can_be_opened() {
    let hive = hive();
    let images = json!([{ "media_type": "image/png", "data": Base64::encode_string(PNG) }]);

    let (status, answered) = send(
        &hive.app,
        "PATCH",
        &format!("/api/v1/decisions/{}/resolution", hive.decision),
        Some(json!({ "action": "Change it", "note": "Like this", "images": images })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The page shows the answered card from this reply, before any refresh.
    let answered: Value = serde_json::from_slice(&answered).unwrap();
    assert_eq!(answered["resolution_action"], "Change it");
    assert_eq!(answered["images"].as_array().unwrap().len(), 1);

    let (status, listed) = send(&hive.app, "GET", "/api/v1/decisions", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Value = serde_json::from_slice(&listed).unwrap();
    let images = listed[0]["images"].as_array().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0]["media_type"], "image/png");
    assert!(images[0]["clarification_id"].is_null());
    let image_id = images[0]["id"].as_str().unwrap();

    let opened = hive
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/decisions/{}/images/{image_id}",
                    hive.decision
                ))
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(opened.status(), StatusCode::OK);
    assert_eq!(opened.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(opened.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(opened.headers()[header::CACHE_CONTROL], "no-store");
    let body = axum::body::to_bytes(opened.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), PNG);

    // Only as part of the request it went with.
    let (status, _) = send(
        &hive.app,
        "GET",
        &format!(
            "/api/v1/decisions/{}/images/{image_id}",
            swarm_domain::DecisionRequestId::new()
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_refused_answer_keeps_none_of_its_files() {
    let hive = hive();
    let images = json!([{ "media_type": "image/png", "data": Base64::encode_string(PNG) }]);

    let (status, _) = send(
        &hive.app,
        "PATCH",
        &format!("/api/v1/decisions/{}/resolution", hive.decision),
        Some(json!({ "action": "Not offered", "images": images })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(files_in(&hive.root).is_empty());
    assert!(
        hive.store
            .decision_images(&[hive.decision])
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_question_with_a_screenshot_is_recorded_once_however_often_it_is_sent() {
    let hive = hive();
    let id = swarm_domain::DecisionClarificationId::new();
    let body = json!({
        "id": id,
        "question": "Is it this one?",
        "images": [{ "media_type": "image/jpeg", "data": Base64::encode_string(JPEG) }],
    });
    let uri = format!("/api/v1/decisions/{}/clarifications", hive.decision);

    for _ in 0..2 {
        let (status, saved) = send(&hive.app, "POST", &uri, Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK);
        let saved: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(saved["image_count"], 1);
    }

    let recorded = hive.store.decision_images(&[hive.decision]).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].clarification_id, Some(id));
    assert_eq!(
        files_in(&hive.root),
        vec![format!("{}.jpg", recorded[0].id)],
        "the repeat's own file is not kept"
    );
}

#[tokio::test]
async fn four_full_size_screenshots_fit_in_one_question_and_in_one_answer() {
    // Each is as large as one image may be, so four of them, base64-encoded,
    // are about 21 MiB: ten times the body any other route accepts.
    let mut largest =
        vec![0_u8; usize::try_from(swarm_persistence::MAX_DECISION_IMAGE_BYTES).unwrap()];
    largest[..PNG.len()].copy_from_slice(PNG);
    let four = json!(vec![
        json!({ "media_type": "image/png", "data": Base64::encode_string(&largest) });
        4
    ]);
    let hive = hive();

    let (status, _) = send(
        &hive.app,
        "POST",
        &format!("/api/v1/decisions/{}/clarifications", hive.decision),
        Some(json!({
            "id": swarm_domain::DecisionClarificationId::new(),
            "question": "Which of these?",
            "images": four,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &hive.app,
        "PATCH",
        &format!("/api/v1/decisions/{}/resolution", hive.decision),
        Some(json!({ "action": "Change it", "images": four })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let recorded = hive.store.decision_images(&[hive.decision]).unwrap();
    assert_eq!(recorded.len(), 8);
    assert!(
        recorded
            .iter()
            .all(|image| image.byte_len == swarm_persistence::MAX_DECISION_IMAGE_BYTES)
    );
    assert_eq!(files_in(&hive.root).len(), 8);
}

#[tokio::test]
async fn a_hive_without_image_storage_refuses_images_and_still_takes_answers() {
    let store = TaskStore::in_memory().unwrap();
    let queen = store.ensure_queen("/fictional/queen").unwrap();
    let decision = store
        .create_decision_request(&swarm_persistence::NewDecisionRequest {
            requesting_worker_id: queen.id,
            task_id: None,
            kind: swarm_domain::DecisionRequestKind::Input,
            urgency: swarm_domain::DecisionUrgency::Normal,
            title: "Which fictional layout?",
            summary: "Choose the layout.",
            reason: "Contract test",
            risk: "No real work",
            evidence: "Fixture only",
            suggested_action: "Keep it",
            allowed_actions: &["Keep it".into()],
            operator_actions: &[],
            questions: &[],
            deadline: None,
            requested_command: None,
            requested_script: None,
        })
        .unwrap()
        .id;
    let app = crate::router(
        AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store.clone()),
    );
    let uri = format!("/api/v1/decisions/{decision}/resolution");

    let (status, _) = send(
        &app,
        "PATCH",
        &uri,
        Some(json!({
            "action": "Keep it",
            "images": [{ "media_type": "image/png", "data": Base64::encode_string(PNG) }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        store.get_decision_request(decision).unwrap().state,
        swarm_domain::DecisionRequestState::Pending
    );

    let (status, _) = send(&app, "PATCH", &uri, Some(json!({ "action": "Keep it" }))).await;
    assert_eq!(status, StatusCode::OK);
}
