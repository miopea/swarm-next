use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64ct::{Base64UrlUnpadded, Encoding};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{ApiError, AppState};

const OPERATOR_SESSION_COOKIE: &str = "swarm_next_operator_session";
const OPERATOR_SESSION_MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;

pub(super) async fn get_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

pub(super) async fn create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let cookie = browser_session_set_cookie(&state, &headers)?;
    Ok((
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (header::SET_COOKIE, cookie),
        ],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

pub(super) async fn delete_session() -> Response {
    (
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::SET_COOKIE,
                HeaderValue::from_static(
                    "swarm_next_operator_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
                ),
            ),
        ],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

pub(super) fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = state.operator_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        )
    })?;
    let presented_bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let expected_session = browser_session_value(expected);
    let presented_session = cookie_value(headers, OPERATOR_SESSION_COOKIE).unwrap_or_default();
    let bearer_matches = presented_bearer.len() == expected.len()
        && bool::from(presented_bearer.as_bytes().ct_eq(expected.as_bytes()));
    let session_matches = presented_session.len() == expected_session.len()
        && bool::from(
            presented_session
                .as_bytes()
                .ct_eq(expected_session.as_bytes()),
        );
    if !bearer_matches && !session_matches {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_operator_token",
            "a valid operator session is required",
        ));
    }
    Ok(())
}

fn browser_session_set_cookie(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<HeaderValue, ApiError> {
    let expected = state.operator_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        )
    })?;
    let secure = if request_is_secure(headers) {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{OPERATOR_SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={OPERATOR_SESSION_MAX_AGE_SECONDS}{secure}",
        browser_session_value(expected),
    ))
    .map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operator_session_unavailable",
            "browser session could not be created",
        )
    })
}

fn browser_session_value(operator_token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.operator-session.v1\0");
    digest.update(operator_token.as_bytes());
    Base64UrlUnpadded::encode_string(&digest.finalize())
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(name)?.strip_prefix('='))
}

fn request_is_secure(headers: &HeaderMap) -> bool {
    if headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
    {
        return true;
    }
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|host| {
            !host.starts_with("localhost")
                && !host.starts_with("127.0.0.1")
                && !host.starts_with("[::1]")
        })
}
