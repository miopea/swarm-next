use std::sync::Arc;

use axum::extract::Path as AxumPath;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::attachments::AttachmentError;
use crate::{ApiError, AppState, attachment_error, authorize, task_store, task_store_error};

#[derive(Debug, Deserialize)]
pub(super) struct DogfoodReportsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateDogfoodReportRequest {
    expectation: String,
    observation: String,
    diagnostic_bundle: String,
    attachment_name: Option<String>,
}

#[derive(Serialize)]
struct DogfoodAttachmentResponse {
    name: String,
}

pub(super) async fn list_reports(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DogfoodReportsQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let reports = task_store(&state)?
        .list_dogfood_reports(query.limit.unwrap_or(20))
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(reports)).into_response())
}

pub(super) async fn create_report(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateDogfoodReportRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let report = task_store(&state)?
        .create_dogfood_report(
            &request.expectation,
            &request.observation,
            &request.diagnostic_bundle,
            request.attachment_name.as_deref(),
        )
        .map_err(|error| task_store_error(&error))?;
    Ok((StatusCode::CREATED, Json(report)).into_response())
}

/// Whether this Hive can file a report anywhere, so the UI can stop implying it.
pub(super) async fn github_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let repository = state
        .github_feedback
        .as_ref()
        .map(crate::github_feedback::GithubFeedback::repository);
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "configured": repository.is_some(),
            "repository": repository,
            // REPORTED SEPARATELY, because they are separate decisions. Filing
            // feedback is what every Swarm user does; receiving a repository's
            // whole issue list is what its maintainer does. A single flag for
            // both is how "I have a token" came to stand for "I triage this
            // repo", and a reader could not tell which they had.
            "issue_intake": state.github_issue_intake_repository().is_some(),
            "issue_intake_repository": state.github_issue_intake_repository(),
        })),
    )
        .into_response())
}

/// Starts connecting a person's own GitHub account, and returns the code to type.
///
/// FRICTIONLESS IS THE REQUIREMENT, and this is the least-friction shape that
/// still ends with a real account: no password, no token to paste, no app for
/// the operator to register, and it behaves the same on a phone. Nothing here
/// blocks filing — a person who never finishes this still files anonymously.
pub(super) async fn github_connect_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let invitation = crate::github_device::invite(state.github_client_id())
        .await
        .map_err(device_error)?;
    // The device code is the secret half and stays on the server. Sending it to
    // the browser would put a credential in a place it is not needed.
    state.remember_pending_github_device(&invitation.device_code);
    Ok((
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "user_code": invitation.user_code,
            "verification_uri": invitation.verification_uri,
            "expires_in": invitation.expires_in,
            "interval": invitation.interval,
        })),
    )
        .into_response())
}

/// Asks once whether they have finished authorising, and stores the result.
///
/// Answers "waiting" rather than failing while they are still typing: GitHub
/// reports that as an error field on a 200, and a client that treats it as one
/// abandons an authorisation the person is halfway through.
pub(super) async fn github_connect_claim(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let Some(device_code) = state.pending_github_device() else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "github_connect_not_started",
            "no GitHub connection is in progress on this Hive",
        ));
    };
    let outcome = crate::github_device::claim(state.github_client_id(), &device_code)
        .await
        .map_err(device_error)?;
    let body = match outcome {
        crate::github_device::DeviceOutcome::Pending => {
            serde_json::json!({ "state": "waiting" })
        }
        crate::github_device::DeviceOutcome::SlowDown { interval } => {
            serde_json::json!({ "state": "waiting", "interval": interval })
        }
        crate::github_device::DeviceOutcome::Denied => {
            state.forget_pending_github_device();
            serde_json::json!({ "state": "declined" })
        }
        crate::github_device::DeviceOutcome::Expired => {
            state.forget_pending_github_device();
            serde_json::json!({ "state": "expired" })
        }
        crate::github_device::DeviceOutcome::Granted(tokens) => {
            let login = crate::github_device::whoami(&tokens.access_token)
                .await
                .map_err(device_error)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
            let store = task_store(&state)?;
            store
                .save_github_connection(
                    &login,
                    &tokens.access_token,
                    tokens.expires_in.map(|seconds| now + seconds),
                    tokens.refresh_token.as_deref(),
                    tokens.refresh_token_expires_in.map(|seconds| now + seconds),
                )
                .map_err(|error| task_store_error(&error))?;
            state.forget_pending_github_device();
            serde_json::json!({ "state": "connected", "login": login })
        }
    };
    Ok((
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response())
}

/// Which account this Hive files as, if any. The token never leaves the server.
pub(super) async fn github_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let store = task_store(&state)?;
    // REFRESHES AS A SIDE EFFECT, deliberately. Opening the feedback dialog asks
    // this, so a connection that expired overnight is renewed before the person
    // types a word — which is the "refreshed without the person doing anything"
    // half of the requirement. The other half is the `lapsed` flag below.
    let state_of_it = usable_github_connection(&state, store).await?;
    let (connected, lapsed, login) = match state_of_it {
        Some(ConnectionState::Usable { login, .. }) => (true, false, Some(login)),
        // Says WHOSE connection ended. "Not connected" would be true and
        // useless: the person did connect, and needs to know it stopped.
        Some(ConnectionState::Lapsed { login }) => (false, true, Some(login)),
        None => (false, false, None),
    };
    Ok((
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "connected": connected,
            "lapsed": lapsed,
            // The login and nothing else. A token in a response body is a token
            // in a browser's memory, a proxy log and a screenshot.
            "login": login,
        })),
    )
        .into_response())
}

/// Disconnects, so filing falls back to anonymous.
pub(super) async fn github_disconnect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let store = task_store(&state)?;
    store
        .forget_github_connection()
        .map_err(|error| task_store_error(&error))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The connected account's token, refreshed if it was about to lapse.
///
/// THE WHOLE POINT OF CONNECTING IS THE ANSWER COMING BACK, and an expired
/// token quietly turns that off: filing keeps working, reports keep reaching
/// GitHub, and the person simply stops hearing anything. Nothing looks broken.
/// So expiry is handled here, in the one place both filing and the status
/// endpoint pass through, rather than left to whichever caller remembers.
///
/// Measured rather than assumed: the operator's own grant came back with an
/// access token good for eight hours and a refresh token good for six months,
/// so this is a live clock and not a hypothetical.
enum ConnectionState {
    Usable {
        login: String,
        token: String,
    },
    /// Somebody connected and it is over. The row is KEPT so the interface can
    /// say whose connection ended, instead of pretending there never was one.
    Lapsed {
        login: String,
    },
}

/// Refreshed a minute EARLY rather than on the stroke: a token that expires
/// while a request is in flight fails for the person holding it.
const REFRESH_SKEW_SECONDS: i64 = 60;

/// Whether a stored access token can still be used.
///
/// A pure function so the decision that triggers a refresh can be tested at
/// all: the refresh itself needs GitHub, and the thing most likely to be wrong
/// is not the HTTP call but WHEN it is made. Getting the skew backwards, or
/// treating a missing expiry as expired, both fail silently in the direction of
/// this whole defect — a person who thinks they are connected and is not.
fn access_token_is_still_good(expires_at: Option<i64>, now: i64) -> bool {
    // No expiry means the app does not expire user tokens. Not the case for the
    // app this ships with, but a fork can register its own with the setting off
    // and must not be forced through a pointless refresh.
    expires_at.is_none_or(|expires| expires > now + REFRESH_SKEW_SECONDS)
}

async fn usable_github_connection(
    state: &AppState,
    store: &swarm_persistence::TaskStore,
) -> Result<Option<ConnectionState>, ApiError> {
    let Some(held) = store
        .github_connection()
        .map_err(|error| task_store_error(&error))?
    else {
        return Ok(None);
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
    let still_good = access_token_is_still_good(held.access_expires_at, now);
    if still_good {
        return Ok(Some(ConnectionState::Usable {
            login: held.login,
            token: held.access_token,
        }));
    }
    let Some(refresh_token) = held.refresh_token.as_deref() else {
        return Ok(Some(ConnectionState::Lapsed { login: held.login }));
    };
    match crate::github_device::refresh(state.github_client_id(), refresh_token).await {
        Ok(tokens) => {
            store
                .save_github_connection(
                    &held.login,
                    &tokens.access_token,
                    tokens.expires_in.map(|seconds| now + seconds),
                    // THE REFRESH TOKEN ROTATES. Keeping the old one works once
                    // and dies at the NEXT refresh — a bug that would surface a
                    // week later, on somebody else's Hive.
                    tokens.refresh_token.as_deref(),
                    tokens.refresh_token_expires_in.map(|seconds| now + seconds),
                )
                .map_err(|error| task_store_error(&error))?;
            Ok(Some(ConnectionState::Usable {
                login: held.login,
                token: tokens.access_token,
            }))
        }
        Err(error) => {
            tracing::warn!(?error, "a GitHub connection could not be refreshed");
            Ok(Some(ConnectionState::Lapsed { login: held.login }))
        }
    }
}

/// GitHub's own words, kept, because the two failures a person can cause here —
/// declining, and letting the code expire — are not the same as an app that was
/// registered without device flow, and only GitHub can tell them apart.
fn device_error(error: crate::github_device::DeviceError) -> ApiError {
    match error {
        crate::github_device::DeviceError::Unreachable => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "github_unreachable",
            "GitHub could not be reached",
        ),
        crate::github_device::DeviceError::Refused(message) => {
            ApiError::new(StatusCode::BAD_GATEWAY, "github_refused", message)
        }
    }
}

/// Files a saved report as a GitHub issue and records where it went.
///
/// SEPARATE FROM SAVING, deliberately. The report is written to this Hive
/// first and stays there whatever happens here, so a GitHub outage cannot lose
/// somebody's words — which is the failure the whole feature exists to end.
pub(super) async fn file_on_github(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(report_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let Some(github) = state.github_feedback.as_ref() else {
        // Says which. "Not configured" and "GitHub refused" send a reader to
        // completely different places, and collapsing them is how the original
        // defect stayed invisible for so long.
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "github_feedback_unconfigured",
            "this Hive has no GitHub credential, so reports stay on it",
        ));
    };
    let store = task_store(&state)?;
    let report = store
        .list_dogfood_reports(swarm_persistence::MAX_DOGFOOD_REPORTS)
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .find(|candidate| candidate.id == report_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "dogfood_report_not_found",
                "that report is not on this Hive",
            )
        })?;
    if let Some(existing) = report.github_issue_url.as_deref() {
        // Already filed. Returning it rather than opening a second issue: a
        // double tap must not scatter duplicates across the tracker.
        return Ok((
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "issue_url": existing, "created": false })),
        )
            .into_response());
    }
    // WHOSE CREDENTIAL FILES THIS decides who hears back when it is closed, so
    // it is read here rather than defaulted. A connected account authors its own
    // issue and GitHub notifies them; without one the report is filed
    // anonymously on this Hive's credential and says so in its own body.
    let connection = usable_github_connection(&state, store).await?;
    let as_user = match connection.as_ref() {
        Some(ConnectionState::Usable { token, .. }) => Some(token.as_str()),
        // A LAPSED CONNECTION STILL FILES — losing somebody's words would be
        // worse — but anonymously, and the response says so, so nobody is left
        // believing an answer is coming that never will.
        Some(ConnectionState::Lapsed { .. }) | None => None,
    };
    let lapsed_login = match &connection {
        Some(ConnectionState::Lapsed { login }) => Some(login.clone()),
        _ => None,
    };
    let issue_url = github
        .file(&report, as_user)
        .await
        .map_err(|error| match error {
            crate::github_feedback::GithubError::Refused(message) => {
                ApiError::new(StatusCode::BAD_GATEWAY, "github_refused", message)
            }
            crate::github_feedback::GithubError::Unreachable => ApiError::new(
                StatusCode::BAD_GATEWAY,
                "github_unreachable",
                "GitHub could not be reached; the report is still saved on this Hive",
            ),
        })?;
    let recorded = store
        .record_dogfood_report_issue(&report_id, &issue_url)
        .map_err(|error| task_store_error(&error))?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "issue_url": recorded.github_issue_url,
            "created": true,
            // NAMED WHEN IT HAPPENS, because this is the moment the person finds
            // out. They connected in order to hear back; if the connection
            // lapsed between then and now, this report went out anonymously and
            // no reply will ever reach them. Silence here is the defect.
            "filed_anonymously_after_lapse": lapsed_login,
        })),
    )
        .into_response())
}

pub(super) async fn upload_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let path = store
        .save(media_type, &body)
        .await
        .map_err(attachment_error)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "attachment_path_unavailable",
                "private attachment name is not valid UTF-8",
            )
        })?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(DogfoodAttachmentResponse { name: name.into() }),
    )
        .into_response())
}

pub(super) async fn download_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let referenced = task_store(&state)?
        .dogfood_attachment_is_referenced(&name)
        .map_err(|error| task_store_error(&error))?;
    if !referenced {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "feedback_attachment_not_found",
            "the private report attachment was not found",
        ));
    }
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let (bytes, media_type) = store.read(&name).await.map_err(attachment_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type)
            .map_err(|_| attachment_error(AttachmentError::Unavailable))?,
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename={name}"))
            .map_err(|_| attachment_error(AttachmentError::Unavailable))?,
    );
    Ok((response_headers, bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::{REFRESH_SKEW_SECONDS, access_token_is_still_good};

    /// The decision that triggers a refresh, which is the part most likely to be
    /// wrong and the only part testable without GitHub.
    #[test]
    fn a_token_is_refreshed_before_it_expires_rather_than_after() {
        let now = 1_788_051_000;

        // Comfortably alive.
        assert!(access_token_is_still_good(Some(now + 3_600), now));
        // Already gone.
        assert!(!access_token_is_still_good(Some(now - 1), now));

        // AND THE SKEW IS THE POINT. A token expiring inside the next minute is
        // treated as spent, because one that lapses mid-request fails for the
        // person holding it and there is nothing to gain by cutting it fine.
        assert!(!access_token_is_still_good(
            Some(now + REFRESH_SKEW_SECONDS - 1),
            now
        ));
        assert!(access_token_is_still_good(
            Some(now + REFRESH_SKEW_SECONDS + 1),
            now
        ));

        // No expiry is not "expired". Reading it that way would send every
        // request through a refresh that cannot succeed, on any app registered
        // without token expiry.
        assert!(access_token_is_still_good(None, now));
    }
}
