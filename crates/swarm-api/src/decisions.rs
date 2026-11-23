use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_terminal::TerminalSize;

use super::{
    ApiError, AppState, application_error, authorize, parse_decision_id, task_service, task_store,
    task_store_error, worker_runtime,
};

#[derive(Debug, Deserialize)]
pub(super) struct ResolveDecisionRequest {
    /// The chosen button. Empty when answering an interview, which has none.
    #[serde(default)]
    action: String,
    /// Answers keyed by question header. Present makes this an interview
    /// answer rather than a ruling.
    #[serde(default)]
    answers: std::collections::BTreeMap<String, Vec<String>>,
    /// Exact rendered question snapshot. Missing is legacy, not confirmation
    /// that this client displayed option descriptions.
    #[serde(default)]
    questions: Option<Vec<swarm_domain::DecisionQuestion>>,
    #[serde(default)]
    note: String,
    /// Which control the operator used, so a disputed resolution can be traced
    /// to where it came in. Reported by the client and recorded as given; it
    /// answers "where did this arrive from", not "who was allowed to send it".
    #[serde(default)]
    surface: String,
    /// Images the operator sent with this answer (ADR 0174).
    #[serde(default)]
    images: Vec<crate::decision_images::SentImage>,
}

pub(super) async fn list_decisions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decisions = task_service(&state)?
        .operator_decision_inbox()
        .map_err(application_error)?;
    let ids: Vec<_> = decisions.iter().map(|entry| entry.decision.id).collect();
    let mut images = task_store(&state)?
        .decision_images(&ids)
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .fold(
            std::collections::HashMap::<_, Vec<_>>::new(),
            |mut by_decision, image| {
                by_decision
                    .entry(image.decision_id)
                    .or_default()
                    .push(image);
                by_decision
            },
        );
    let listed: Vec<_> = decisions
        .iter()
        .map(|entry| WithImages {
            images: images.remove(&entry.decision.id).unwrap_or_default(),
            entry,
        })
        .collect();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(listed)).into_response())
}

/// A request as the operator's page reads it, with the images the operator
/// sent on it (ADR 0174). The images are added here rather than on the shared
/// record so the record a worker engine reads does not change.
#[derive(serde::Serialize)]
struct WithImages<'a, T> {
    #[serde(flatten)]
    entry: &'a T,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<swarm_persistence::DecisionImage>,
}

/// The script a command card runs and what Swarm kept of it, for the
/// operator reading the card (ADR 0187). Fetched when they open it, so the
/// list of cards does not carry every script's text.
pub(super) async fn decision_script(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decision_id = parse_decision_id(&decision_id)?;
    let script = task_service(&state)?
        .command_card_script(decision_id)
        .map_err(application_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "script": script.as_ref().map(script_json) })),
    )
        .into_response())
}

/// A card's script as the operator's page and Queen's list both read it:
/// its path, the sha256 the card pins, and its text or why none was kept.
pub(super) fn script_json(script: &swarm_application::CommandCardScript) -> serde_json::Value {
    let mut value = serde_json::json!({ "path": script.path, "sha256": script.sha256 });
    if let Some(text) = &script.text {
        value["text"] = text.as_str().into();
    }
    if let Some(withheld) = &script.withheld {
        value["withheld"] = withheld.as_str().into();
    }
    value
}

/// One image the operator sent with an answer or question on this request.
pub(super) async fn decision_image(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((decision_id, image_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decision_id = parse_decision_id(&decision_id)?;
    let image_id = image_id.parse::<uuid::Uuid>().map_err(|_| {
        crate::decision_images::decision_image_error(
            crate::decision_images::DecisionImageError::NotFound,
        )
    })?;
    crate::decision_images::image_response(&state, task_store(&state)?, decision_id, image_id).await
}

pub(super) async fn resolve_decision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision_id): Path<String>,
    Json(request): Json<ResolveDecisionRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decision_id = parse_decision_id(&decision_id)?;
    let service = task_service(&state)?;
    let written =
        crate::decision_images::write_sent(&state, task_store(&state)?, &request.images).await?;
    // Answers and an action are different resolutions of different record
    // shapes, and the store rejects each against the wrong one, so this only
    // has to route.
    let resolved = if request.answers.is_empty() {
        service.resolve_operator_decision_with_images(
            decision_id,
            &request.action,
            &request.note,
            &request.surface,
            &written.images,
        )
    } else {
        service.answer_operator_decision_from_snapshot(
            decision_id,
            &request.answers,
            &request.note,
            &request.surface,
            request.questions.as_deref(),
            &written.images,
        )
    };
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            crate::decision_images::discard(&state, &written.images).await;
            return Err(application_error(error));
        }
    };
    let sent_images = written.images.len();
    // Recorded: the next write may now sweep. Not held across the wake and
    // delivery below, which can take a while.
    drop(written);
    tracing::info!(
        decision_id = %decision_id,
        action = %if request.answers.is_empty() { request.action.as_str() } else { "answered" },
        answered_questions = request.answers.len(),
        images = sent_images,
        surface = %if request.surface.is_empty() { "unreported" } else { &request.surface },
        "operator resolved a decision"
    );
    // Answering a worker is an explicit operator action. Wake that exact worker
    // so the durable reply does not sit indefinitely behind a sleeping process.
    // A failed wake remains visible on the worker and the queued reply stays
    // durable for a later retry; the recorded operator decision is never lost.
    if let Err(error) = worker_runtime::start_worker_process(
        &state,
        resolved.requesting_worker_id,
        TerminalSize::default(),
    )
    .await
    {
        state
            .worker_errors
            .write()
            .await
            .insert(resolved.requesting_worker_id, error.message.clone());
        tracing::warn!(
            decision_id = %decision_id,
            worker_id = %resolved.requesting_worker_id,
            message = %error.message,
            "decision requester could not be woken after operator resolution"
        );
    }
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let store = task_store(&state)?;
    let decision = store
        .get_decision_request(decision_id)
        .map_err(|error| task_store_error(&error))?;
    // The page puts this in place of the card it answered, so it carries what
    // was sent with it, as the inbox listing does.
    let images = store
        .decision_images(&[decision_id])
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(WithImages {
        entry: &decision,
        images,
    })
    .into_response())
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::{Value, json};
    use swarm_persistence::{GRANT_COMMAND_ACTION, NewDecisionRequest, TaskStore};
    use swarm_terminal::HostClient;
    use tower::ServiceExt as _;

    use crate::AppState;

    async fn send(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
        auth: bool,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if auth {
            request = request.header("authorization", "Bearer secret");
        }
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
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// ADR 0187 over the operator's routes: the card's script is read when
    /// the card is opened, with the text kept when it was filed, and an Allow
    /// of a script that changed since is refused with nothing granted.
    #[tokio::test]
    async fn the_operator_reads_a_cards_script_and_an_allow_of_a_changed_one_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("release.sh");
        let body = "#!/bin/sh\nexec ./deploy --slot staging\n";
        std::fs::write(&path, body).unwrap();
        let path = path.to_str().unwrap().to_owned();

        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/fictional/queen").unwrap();
        store
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .unwrap();
        let task = store.create_task("Release it", "/fictional/app").unwrap();
        let filed = swarm_application::read_script(&path).unwrap();
        let card = |script: Option<&swarm_persistence::FiledScript>| {
            store
                .create_decision_request(&NewDecisionRequest {
                    requesting_worker_id: queen.id,
                    task_id: Some(task.id),
                    kind: swarm_domain::DecisionRequestKind::Approval,
                    urgency: swarm_domain::DecisionUrgency::Normal,
                    title: "Run the release script",
                    summary: "The classifier denied it.",
                    reason: "Needed for the release.",
                    risk: "Staging only.",
                    evidence: "Fixture only.",
                    suggested_action: "",
                    allowed_actions: &[],
                    operator_actions: &[],
                    questions: &[],
                    deadline: None,
                    requested_command: Some(&path),
                    requested_script: script,
                })
                .unwrap()
                .id
        };
        let with_script = card(Some(&filed));
        let without_script = card(None);
        let app = crate::router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store.clone()),
        );
        let uri = |id: swarm_domain::DecisionRequestId| format!("/api/v1/decisions/{id}/script");

        let (status, read) = send(&app, "GET", &uri(with_script), None, true).await;
        assert_eq!(status, StatusCode::OK, "{read}");
        assert_eq!(
            read,
            json!({"script": {"path": path, "sha256": filed.script.sha256, "text": body}})
        );
        let (status, _) = send(&app, "GET", &uri(with_script), None, false).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, read) = send(&app, "GET", &uri(without_script), None, true).await;
        assert_eq!((status, read), (StatusCode::OK, json!({"script": null})));
        let (status, _) = send(
            &app,
            "GET",
            &uri(swarm_domain::DecisionRequestId::new()),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        std::fs::write(&path, "#!/bin/sh\nexec ./deploy --slot production\n").unwrap();
        let (status, refused) = send(
            &app,
            "PATCH",
            &format!("/api/v1/decisions/{with_script}/resolution"),
            Some(json!({"action": GRANT_COMMAND_ACTION, "surface": "inbox"})),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert!(
            refused
                .to_string()
                .contains("has changed since it was filed"),
            "the operator is told why: {refused}"
        );
        assert!(
            store
                .get_decision_request(with_script)
                .unwrap()
                .resolved_at
                .is_none()
        );
        let (_, read) = send(&app, "GET", &uri(with_script), None, true).await;
        assert_eq!(
            read["script"]["text"], body,
            "the card still shows what was filed"
        );
    }
}
