//! Authenticated Hive multipart adapter; reviewed-file rules remain in the domain.
use crate::ApiError;
use axum::{
    extract::{Multipart, multipart::Field},
    http::StatusCode,
};
use serde::Deserialize;
use std::{collections::HashSet, time::Duration};
use swarm_domain::{
    SUPPORT_ATTACHMENT_MAX_BYTES, SUPPORT_ATTACHMENT_MAX_COUNT, SUPPORT_ATTACHMENTS_MAX_BYTES,
    SupportAttachment, SupportAttachmentMetadata, SupportSubmissionInput,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    submission: SupportSubmissionInput,
    attachments: Vec<SupportAttachmentMetadata>,
}

pub(super) struct Upload {
    pub submission: SupportSubmissionInput,
    pub files: Vec<SupportAttachment>,
}

fn invalid() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "support_upload_invalid",
        "The upload does not match the reviewed file list. Keep the original report for retry.",
    )
}
fn oversized() -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "support_upload_size",
        "Support accepts up to four files, 5 MiB each and 12 MiB combined.",
    )
}

async fn bounded_bytes(mut field: Field<'_>, maximum: usize) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|_| invalid())? {
        if chunk.len() > maximum.saturating_sub(bytes.len()) {
            return Err(oversized());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) async fn read_with_deadline(
    multipart: Multipart,
    deadline: Duration,
) -> Result<Upload, ApiError> {
    tokio::time::timeout(deadline, read(multipart)).await.map_err(|_| {
        ApiError::new(StatusCode::REQUEST_TIMEOUT, "support_upload_timeout", "Upload did not finish. Retry this exact report; it has not been saved by this request.")
    })?
}

async fn read(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let field = multipart
        .next_field()
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if field.name() != Some("manifest")
        || field.file_name().is_some()
        || field.content_type() != Some("application/json")
    {
        return Err(invalid());
    }
    let manifest: Manifest =
        serde_json::from_slice(&bounded_bytes(field, 128 * 1024).await?).map_err(|_| invalid())?;
    if manifest.attachments.is_empty() || manifest.attachments.len() > SUPPORT_ATTACHMENT_MAX_COUNT
    {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut total = 0usize;
    for file in &manifest.attachments {
        if file.id.is_nil() || !ids.insert(file.id) {
            return Err(invalid());
        }
        if file.size_bytes > SUPPORT_ATTACHMENT_MAX_BYTES {
            return Err(oversized());
        }
        total = total.saturating_add(file.size_bytes);
    }
    if total > SUPPORT_ATTACHMENTS_MAX_BYTES {
        return Err(oversized());
    }
    let mut files = vec![None; manifest.attachments.len()];
    while let Some(field) = multipart.next_field().await.map_err(|_| invalid())? {
        let name = field.name().ok_or_else(invalid)?;
        let position = manifest
            .attachments
            .iter()
            .position(|file| name == format!("file:{}", file.id))
            .ok_or_else(invalid)?;
        let metadata = &manifest.attachments[position];
        if files[position].is_some() || field.content_type() != Some(metadata.media_type.as_str()) {
            return Err(invalid());
        }
        let bytes = bounded_bytes(field, metadata.size_bytes).await?;
        files[position] =
            Some(SupportAttachment::validate(metadata.clone(), bytes).map_err(|_| invalid())?);
    }
    let files = files
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(invalid)?;
    Ok(Upload {
        submission: manifest.submission,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::Request,
    };
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use std::sync::Arc;
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;
    const ID: &str = "00000000-0000-0000-0000-000000000001";
    fn fixture() -> (AppState, TaskStore) {
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default()
            .with_task_store(store.clone())
            .with_central_support("https://admin.example.invalid")
            .unwrap();
        *state.operator_token.write().unwrap() = Some(Arc::from("fixture"));
        (state, store)
    }
    fn manifest() -> serde_json::Value {
        serde_json::json!({"submission": {"submission_key":"00000000-0000-0000-0000-000000000010", "kind":"bug_report", "email":"fictional@example.invalid", "name":null,"subject":"Fixture","body":"Reviewed"},
            "attachments":[{"id":ID,"file_name":"fictional.txt","media_type":"text/plain","size_bytes":3,"sha256":format!("{:x}",Sha256::digest(b"one"))}]})
    }
    fn body(manifest: &serde_json::Value, files: &[(&str, &str)]) -> String {
        let mut body = format!(
            "--fixture\r\nContent-Disposition: form-data; name=\"manifest\"\r\nContent-Type: application/json\r\n\r\n{manifest}\r\n"
        );
        for (id, bytes) in files {
            write!(body,"--fixture\r\nContent-Disposition: form-data; name=\"file:{id}\"; filename=\"fictional.txt\"\r\nContent-Type: text/plain\r\n\r\n{bytes}\r\n").unwrap();
        }
        body.push_str("--fixture--\r\n");
        body
    }
    async fn request(app: Router, authorized: bool, body: Body) -> axum::response::Response {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/feedback/support/attachments")
            .header("content-type", "multipart/form-data; boundary=fixture");
        if authorized {
            request = request.header("authorization", "Bearer fixture");
        }
        app.oneshot(request.body(body).unwrap()).await.unwrap()
    }
    #[tokio::test]
    async fn exact_authenticated_upload_is_saved_once_and_never_sent_by_request() {
        let (state, store) = fixture();
        let app = crate::router(state);
        for _ in 0..2 {
            let response = request(
                app.clone(),
                true,
                Body::from(body(&manifest(), &[(ID, "one")])),
            )
            .await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let result = to_bytes(response.into_body(), 4096).await.unwrap();
            let result = String::from_utf8(result.to_vec()).unwrap();
            assert!(!result.contains("fictional"));
        }
        let statuses = store.support_submission_statuses().unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].delivery.attempts, 0);
        let saved = store
            .support_submission(statuses[0].submission_key.parse().unwrap())
            .unwrap();
        assert_eq!(saved.attachments[0].bytes(), b"one");
        let mut changed = manifest();
        changed["attachments"][0]["sha256"] = format!("{:x}", Sha256::digest(b"two")).into();
        assert_eq!(
            request(app, true, Body::from(body(&changed, &[(ID, "two")])))
                .await
                .status(),
            StatusCode::CONFLICT
        );
    }
    #[tokio::test]
    async fn missing_duplicate_changed_or_unreviewed_parts_never_save_a_partial_report() {
        let (state, store) = fixture();
        let app = crate::router(state);
        for files in [
            vec![],
            vec![(ID, "one"), (ID, "one")],
            vec![(ID, "two")],
            vec![("unknown", "one")],
        ] {
            assert_eq!(
                request(app.clone(), true, Body::from(body(&manifest(), &files)))
                    .await
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        for change in ["destination", "attachments", "size", "manifest_file"] {
            let mut input = manifest();
            if change == "destination" {
                input["destination"] = "https://other.example.invalid".into();
            }
            if change == "attachments" {
                input["attachments"] = serde_json::json!([]);
            }
            if change == "size" {
                input["attachments"][0]["size_bytes"] = (SUPPORT_ATTACHMENT_MAX_BYTES + 1).into();
            }
            let mut encoded = body(&input, &[(ID, "one")]);
            if change == "manifest_file" {
                encoded = encoded.replacen(
                    "name=\"manifest\"",
                    "name=\"manifest\"; filename=\"manifest.json\"",
                    1,
                );
            }
            let response = request(app.clone(), true, Body::from(encoded)).await;
            assert_eq!(
                response.status(),
                if change == "size" {
                    StatusCode::PAYLOAD_TOO_LARGE
                } else {
                    StatusCode::BAD_REQUEST
                }
            );
        }
        assert!(store.support_submission_statuses().unwrap().is_empty());
    }
    #[tokio::test]
    async fn authentication_and_upload_admission_refuse_before_reading_a_stalled_body() {
        let (state, store) = fixture();
        let held = state
            .central_support
            .as_ref()
            .unwrap()
            .uploads
            .clone()
            .acquire_many_owned(2)
            .await
            .unwrap();
        let app = crate::router(state);
        for authorized in [false, true] {
            let body = Body::from_stream(futures_util::stream::pending::<
                Result<Vec<u8>, std::io::Error>,
            >());
            let response = tokio::time::timeout(
                Duration::from_secs(1),
                request(app.clone(), authorized, body),
            )
            .await
            .unwrap();
            assert_eq!(
                response.status(),
                if authorized {
                    StatusCode::TOO_MANY_REQUESTS
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
        }
        drop(held);
        assert_eq!(
            request(app, true, Body::from(body(&manifest(), &[(ID, "one")])))
                .await
                .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(store.support_submission_statuses().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn unfinished_multipart_is_bounded_by_the_owner_deadline() {
        let app = Router::new().route(
            "/api/v1/feedback/support/attachments",
            axum::routing::post(|multipart: Multipart| async {
                match read_with_deadline(multipart, Duration::from_millis(20)).await {
                    Ok(_) => StatusCode::OK,
                    Err(_) => StatusCode::REQUEST_TIMEOUT,
                }
            }),
        );
        let body = Body::from_stream(futures_util::stream::pending::<
            Result<Vec<u8>, std::io::Error>,
        >());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), request(app, true, body))
                .await
                .unwrap()
                .status(),
            StatusCode::REQUEST_TIMEOUT
        );
    }
}
