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

/// Whether this request genuinely arrived over HTTPS.
///
/// Only the forwarded protocol answers this. It used to infer HTTPS from the
/// host — anything that was not localhost was assumed secure — and that is
/// exactly backwards for the way Swarm is reached from a second device: over
/// plain HTTP at a LAN address or a machine name.
///
/// The cost was invisible and repeated. A `Secure` cookie is refused by the
/// browser on an HTTP origin, so signing in appeared to work, the session
/// cookie was silently dropped, and the next page load was back at the unlock
/// panel. The operator hit it three times before it was diagnosed, each time
/// reading as "something didn't come back up".
///
/// Swarm binds plain HTTP on localhost and the documented way to expose it is
/// an HTTPS proxy in front, which sets this header. So the header is not a
/// hint about the truth here — it is the whole truth.
fn request_is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
}

#[cfg(test)]
mod secure_cookie_tests {
    use super::request_is_secure;
    use axum::http::{HeaderMap, HeaderValue, header};

    fn headers(host: &str, forwarded: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        if let Some(proto) = forwarded {
            headers.insert("x-forwarded-proto", HeaderValue::from_str(proto).unwrap());
        }
        headers
    }

    /// The operator reaches Swarm from a second device at a LAN address or a
    /// machine name, over plain HTTP. Marking that cookie `Secure` means the
    /// browser refuses to store it: sign-in appears to work and the next page
    /// load is back at the unlock panel, with nothing wrong on the server.
    #[test]
    fn plain_http_from_another_device_is_not_treated_as_secure() {
        assert!(!request_is_secure(&headers("192.168.1.42:8766", None)));
        assert!(!request_is_secure(&headers("bgs-development:8766", None)));
        assert!(!request_is_secure(&headers("swarm.local:8766", None)));
        // Localhost was already handled; it must stay handled.
        assert!(!request_is_secure(&headers("127.0.0.1:8766", None)));
        assert!(!request_is_secure(&headers("localhost:8766", None)));
    }

    /// An HTTPS proxy in front is the documented way to expose Swarm, and it
    /// says so in the one header that actually knows.
    #[test]
    fn a_proxy_reporting_https_is_secure() {
        assert!(request_is_secure(&headers(
            "swarm.example.com",
            Some("https")
        )));
        assert!(request_is_secure(&headers(
            "swarm.example.com",
            Some("HTTPS")
        )));
        assert!(!request_is_secure(&headers(
            "swarm.example.com",
            Some("http")
        )));
    }
}
