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
    let cookie = browser_session_cookie(&state, &headers)?;
    Ok((
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (header::SET_COOKIE, cookie),
        ],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct RotateTokenRequest {
    token: String,
}

/// Replaces the operator token, in memory and in the file systemd reads.
///
/// Both, deliberately. Memory alone would revert on the next restart; the file
/// alone would not take effect until one. Writing both means the value the
/// operator just chose is the value in force now and after a reboot.
///
/// Every browser session dies with it, including the one making this request.
/// That is what rotating means and the control room says so before asking.
pub(super) async fn rotate_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(request): axum::Json<RotateTokenRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let token = request.token.trim();
    // Long enough not to be guessed, and printable so it survives being copied
    // between a password manager, a terminal and a phone — which is how the
    // last one went wrong.
    if token.len() < 16 || token.len() > 200 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "operator_token_length",
            "an operator token must be between 16 and 200 characters",
        ));
    }
    if token.bytes().any(|byte| !(b'!'..=b'~').contains(&byte)) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "operator_token_characters",
            "an operator token must be printable characters without spaces",
        ));
    }
    let config = state.operator_config_path().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "operator_config_unavailable",
            "this Hive does not know where its configuration lives",
        )
    })?;
    persist_operator_token(&config, token)?;
    if !state.replace_operator_token(token) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        ));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Rewrites only the token line, leaving every other setting untouched.
///
/// Written to a neighbouring file and renamed, so an interrupted write cannot
/// leave the Hive with a configuration it cannot start from.
fn persist_operator_token(config: &std::path::Path, token: &str) -> Result<(), ApiError> {
    let unavailable = || {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operator_token_not_persisted",
            "the new token could not be written to this Hive's configuration",
        )
    };
    let existing = std::fs::read_to_string(config).map_err(|_| unavailable())?;
    let mut replaced = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            if line.starts_with("SWARM_OPERATOR_TOKEN=") {
                replaced = true;
                format!("SWARM_OPERATOR_TOKEN={token}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !replaced {
        lines.push(format!("SWARM_OPERATOR_TOKEN={token}"));
    }
    let mut body = lines.join("\n");
    body.push('\n');

    let temporary = config.with_extension("env.rotating");
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| unavailable())?;
        file.write_all(body.as_bytes()).map_err(|_| unavailable())?;
        file.sync_all().map_err(|_| unavailable())?;
    }
    std::fs::rename(&temporary, config).map_err(|_| unavailable())
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

/// Whether this request may proceed without a credential.
///
/// The operator's ruling: "localhost is not public, it shouldn't need any
/// tokens." True of the network, and not quite true of the browser — a page on
/// any site can make requests to 127.0.0.1 from the operator's machine, and on
/// this installation localhost is forwarded from Windows into WSL, so "local"
/// is a larger set than it sounds.
///
/// So both must hold: the request arrived at a literal loopback address, and it
/// carries no `Origin` from somewhere else. A drive-by from another site fails
/// the second test even though it passes the first, and a proxied request fails
/// the first because the forwarded host is not loopback.
fn is_trusted_loopback(headers: &HeaderMap) -> bool {
    if headers.get("x-forwarded-proto").is_some() || headers.get("x-forwarded-host").is_some() {
        return false;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    let loopback = name == "localhost" || name == "127.0.0.1" || name == "[::1]" || name == "::1";
    if !loopback {
        return false;
    }
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(origin) => {
            let origin = origin
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            let origin_host = origin.rsplit_once(':').map_or(origin, |(name, _)| name);
            origin_host == name
        }
    }
}

pub(super) fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if is_trusted_loopback(headers) {
        return Ok(());
    }
    let expected = state.operator_token().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        )
    })?;
    let expected = expected.as_ref();
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
        // Naming which one failed is the difference between "my token is
        // wrong" and "my browser dropped the cookie", which read identically
        // for an hour while the operator was locked out of their own Hive.
        let (code, message) = if !presented_bearer.is_empty() {
            (
                "invalid_operator_token",
                "that operator token does not match this Hive",
            )
        } else if !presented_session.is_empty() {
            (
                "expired_operator_session",
                "this browser session is no longer valid — sign in again",
            )
        } else {
            (
                "operator_session_required",
                "a valid operator session is required",
            )
        };
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, code, message));
    }
    Ok(())
}

pub(super) fn browser_session_cookie(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<HeaderValue, ApiError> {
    let expected = state.operator_token().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        )
    })?;
    let expected = expected.as_ref();
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

/// Whether this request arrived over HTTPS.
///
/// Secure by default, and insecure only where that is demonstrable. A cookie
/// wrongly marked `Secure` is silently discarded by the browser and the
/// operator cannot sign in; a cookie wrongly left unmarked travels in clear
/// text. The first is an outage, the second is a leak, so the default has to be
/// the safe one and the exceptions have to be specific.
///
/// A forwarded protocol is believed outright — Swarm binds plain HTTP and the
/// documented way to expose it is a proxy in front, which is how this
/// installation reaches swarm2.bfgsolutions.net through a Cloudflare tunnel.
///
/// Otherwise the host decides, and only addresses that cannot be reached over
/// HTTPS count as insecure: loopback, a private range, or a bare name with no
/// dot in it. This is the case that was wrong before — reaching Swarm from a
/// second device at a LAN address or a machine name is plain HTTP, and the old
/// rule called anything that was not localhost secure.
fn request_is_secure(headers: &HeaderMap) -> bool {
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
    {
        return proto.eq_ignore_ascii_case("https");
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let name = host.split(':').next().unwrap_or(host);
    !is_plain_http_host(name)
}

/// Hosts that cannot be serving HTTPS with a certificate anyone trusts.
fn is_plain_http_host(name: &str) -> bool {
    if name == "localhost" || name.ends_with(".localhost") {
        return true;
    }
    if name == "::1" || name == "[::1]" {
        return true;
    }
    // A .local name is mDNS on the same network, which cannot hold a
    // certificate anyone trusts.
    if name
        .rsplit_once('.')
        .is_some_and(|(_, tld)| tld.eq_ignore_ascii_case("local"))
    {
        return true;
    }
    // A bare machine name, which is how a second device on the same network
    // reaches this Hive.
    if !name.contains('.') {
        return true;
    }
    let octets: Vec<&str> = name.split('.').collect();
    if octets.len() == 4
        && octets
            .iter()
            .all(|part| part.parse::<u8>().is_ok() && !part.is_empty())
    {
        let first: u8 = octets[0].parse().unwrap_or(0);
        let second: u8 = octets[1].parse().unwrap_or(0);
        return first == 127
            || first == 10
            || (first == 192 && second == 168)
            || (first == 172 && (16..=31).contains(&second));
    }
    false
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

#[cfg(test)]
mod loopback_tests {
    use super::is_trusted_loopback;
    use axum::http::{HeaderMap, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    /// "Localhost is not public, it shouldn't need any tokens."
    #[test]
    fn a_request_from_this_machine_needs_no_credential() {
        assert!(is_trusted_loopback(&headers(&[("host", "127.0.0.1:8766")])));
        assert!(is_trusted_loopback(&headers(&[("host", "localhost:8766")])));
        assert!(is_trusted_loopback(&headers(&[("host", "[::1]:8766")])));
        // The page Swarm itself serves is same-origin, and still trusted.
        assert!(is_trusted_loopback(&headers(&[
            ("host", "127.0.0.1:8766"),
            ("origin", "http://127.0.0.1:8766"),
        ])));
    }

    /// "Not public" is not "not reachable". A page on any site can make
    /// requests to 127.0.0.1 from the operator's own browser, so the origin has
    /// to agree before the loopback address means anything.
    #[test]
    fn another_site_driving_this_hive_from_the_browser_is_not_trusted() {
        assert!(!is_trusted_loopback(&headers(&[
            ("host", "127.0.0.1:8766"),
            ("origin", "https://example.com"),
        ])));
        assert!(!is_trusted_loopback(&headers(&[
            ("host", "localhost:8766"),
            ("origin", "http://evil.localhost.attacker.com"),
        ])));
    }

    /// A proxied request is not local however it reaches the socket, which is
    /// how this Hive is published at swarm2.bfgsolutions.net.
    #[test]
    fn a_proxied_request_is_never_trusted_as_loopback() {
        assert!(!is_trusted_loopback(&headers(&[
            ("host", "127.0.0.1:8766"),
            ("x-forwarded-proto", "https"),
        ])));
        assert!(!is_trusted_loopback(&headers(&[
            ("host", "127.0.0.1:8766"),
            ("x-forwarded-host", "swarm2.bfgsolutions.net"),
        ])));
        assert!(!is_trusted_loopback(&headers(&[(
            "host",
            "swarm2.bfgsolutions.net"
        )])));
    }
}
