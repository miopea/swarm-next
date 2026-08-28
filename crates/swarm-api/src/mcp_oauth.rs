//! The OAuth 2.0 authorization server an outside tool talks to before it can
//! reach `/mcp`.
//!
//! WHY THIS EXISTS. `POST /mcp` has been externally reachable and per-principal
//! authenticated for some time, and that made it look finished: an unauthenticated
//! request returns 401, not 404. It is not finished. A remote client had no way
//! to OBTAIN a credential — `GET /mcp` is 405, and every OAuth discovery path
//! answered 404 — so the 401 was a locked door with no key cut for anyone.
//!
//! Legacy Swarm did have this. `swarm-legacy/src/swarm/auth/oauth_server.py`
//! carried 309 lines of it, and its test file names the target in its own
//! docstring: a regression guard for the Claude Desktop "Connect" flow. The
//! connector still sitting in the operator's client is a survivor of that
//! server, and it reads "Reconnect" because the server behind it is gone.
//!
//! Scope and the four settled decisions are in `docs/39-connecting-an-outside-tool.md`.
//! That document is the contract; this module implements it.

use axum::{
    Form, Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64ct::{Base64UrlUnpadded, Encoding};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use subtle::ConstantTimeEq;

use crate::AppState;

/// The scope an outside tool is granted. One scope, because the surface is
/// settled: what a worker can do, minus approve and assign.
pub(crate) const SCOPE: &str = "mcp";

/// Where a client is told to look when it gets a 401.
pub(crate) const PROTECTED_RESOURCE_PATH: &str = "/.well-known/oauth-protected-resource";
pub(crate) const AUTHORIZATION_SERVER_PATH: &str = "/.well-known/oauth-authorization-server";

/// The origin this Hive is reachable at, as a client would address it.
///
/// `SWARM_PUBLIC_BASE_URL` is authoritative when set — it is what the tunnel
/// publishes, and it is the only value that is right when the request arrives
/// through a proxy. Falling back to the request's own `Host` keeps the flow
/// working on loopback, where a developer has no public URL configured and
/// still wants to connect a local client.
///
/// Returning None rather than guessing matters: a discovery document that
/// advertises the WRONG origin sends the client's browser somewhere that
/// cannot complete the flow, and the failure surfaces as a broken redirect
/// rather than as a missing setting.
pub(crate) fn base_url(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if let Some(configured) = state.public_base_url.as_deref() {
        return Some(configured.to_owned());
    }
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    // A forwarded request tells us its scheme; a direct one is loopback http.
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    Some(format!("{scheme}://{host}"))
}

/// The `WWW-Authenticate` value for an unauthenticated `/mcp` request.
///
/// THE `resource_metadata` PARAMETER IS THE WHOLE POINT. A bare `Bearer` tells
/// a client it needs a token and does not tell it where to get one, so the 401
/// is a dead end. With the parameter the same 401 becomes an invitation: the
/// client fetches the named document, finds the authorization server, and
/// starts the flow. That is the difference between "reachable" and
/// "connectable", and it is one header.
pub(crate) fn challenge(base: Option<&str>) -> String {
    match base {
        Some(base) => format!(r#"Bearer resource_metadata="{base}{PROTECTED_RESOURCE_PATH}""#),
        // Nothing to point at. Still a valid challenge, and honest about it.
        None => "Bearer".to_owned(),
    }
}

fn metadata_response(body: Value) -> Response {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            // Discovery is fetched cross-origin by the client's own agent.
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                header::HeaderValue::from_static("*")
                    .to_str()
                    .unwrap_or("*"),
            ),
        ],
        Json(body),
    )
        .into_response()
}

/// RFC 9728 protected-resource metadata.
///
/// PUBLIC ON PURPOSE, like every document under `/.well-known/`. Putting
/// discovery behind the credential it exists to help you obtain is a loop, and
/// the client cannot tell that 401 apart from "this server has no discovery" —
/// which is why legacy marked the whole prefix public and required a 404 for
/// anything absent.
pub(crate) async fn protected_resource(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let Some(base) = base_url(&state, &headers) else {
        return unavailable();
    };
    metadata_response(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "scopes_supported": [SCOPE],
        "bearer_methods_supported": ["header"],
    }))
}

/// RFC 8414 authorization-server metadata.
pub(crate) async fn authorization_server(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let Some(base) = base_url(&state, &headers) else {
        return unavailable();
    };
    metadata_response(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "registration_endpoint": format!("{base}/oauth/register"),
        "scopes_supported": [SCOPE],
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["client_secret_post", "none"],
        // PKCE is required, not offered. A public client without it is
        // interceptable, and every client that reaches this server is public.
        "code_challenge_methods_supported": ["S256"],
    }))
}

/// Said plainly rather than served with a guessed origin.
fn unavailable() -> Response {
    (
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "error": "server_error",
            "error_description":
                "This Hive does not know its own public address. Set SWARM_PUBLIC_BASE_URL, \
                 or reach it through a host header, before connecting an outside tool.",
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Signing
// ---------------------------------------------------------------------------

/// Tokens are signed, not stored.
///
/// NOTHING IN THIS FLOW NEEDS A SCHEMA MIGRATION, which was worth designing for:
/// this repository has shipped a patch release carrying seven migrations and hurt
/// an operator with it. A registration is a random `client_id` plus a secret
/// DERIVED from it, and the redirect URI is re-checked against the allow-list at
/// authorize time rather than remembered — so there is no client table. Access
/// and refresh tokens carry their own claims under a MAC. Only the client
/// principal that names board writes needs storage, and that is a separate step.
///
/// THE KEY IS DERIVED FROM THE OPERATOR TOKEN, deliberately. Rotating that token
/// invalidates every outside tool's connection at once, which gives the operator
/// a single revoke-everything lever and stops a connection outliving the
/// credential it was authorised under.
fn signing_key(state: &AppState) -> Option<[u8; 32]> {
    let token = state.operator_token()?;
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.mcp-oauth.signing.v1\0");
    digest.update(token.as_ref().as_bytes());
    Some(digest.finalize().into())
}

/// HMAC-SHA256, written out because the workspace carries `sha2` and not `hmac`.
///
/// A bare `SHA256(key || message)` would be simpler and wrong: the payload here
/// is attacker-supplied and attacker-visible, which is exactly the shape
/// length-extension exploits.
fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);
    outer.finalize().into()
}

fn sign(key: &[u8; 32], payload: &Value) -> String {
    let body = Base64UrlUnpadded::encode_string(payload.to_string().as_bytes());
    let mac = Base64UrlUnpadded::encode_string(&hmac_sha256(key, body.as_bytes()));
    format!("{body}.{mac}")
}

/// Verifies the MAC in constant time, then the expiry.
fn unsign(key: &[u8; 32], token: &str, now: i64) -> Option<Value> {
    let (body, mac) = token.split_once('.')?;
    let presented = Base64UrlUnpadded::decode_vec(mac).ok()?;
    let expected = hmac_sha256(key, body.as_bytes());
    if presented.len() != expected.len() || !bool::from(presented.ct_eq(&expected)) {
        return None;
    }
    let decoded = Base64UrlUnpadded::decode_vec(body).ok()?;
    let payload: Value = serde_json::from_slice(&decoded).ok()?;
    // A missing `exp` means "does not expire", which is only ever used for a
    // client id. Every token minted by `mint` carries one, so a token cannot
    // reach this branch by losing a field — the MAC covers the whole payload.
    if payload
        .get("exp")
        .is_some_and(|expires_at| expires_at.as_i64().is_none_or(|value| value <= now))
    {
        return None;
    }
    Some(payload)
}

const ACCESS_TOKEN_TTL: i64 = 3600;
const REFRESH_TOKEN_TTL: i64 = 30 * 24 * 3600;
const AUTH_CODE_TTL: i64 = 300;

fn mint(key: &[u8; 32], kind: &str, client_id: &str, ttl: i64, now: i64) -> String {
    sign(
        key,
        &json!({ "typ": kind, "cid": client_id, "scope": SCOPE, "exp": now + ttl }),
    )
}

/// The client id a presented token belongs to, or None if it is not a valid
/// token of that kind.
pub(crate) fn client_for_token(state: &AppState, token: &str, now: i64) -> Option<String> {
    let key = signing_key(state)?;
    let payload = unsign(&key, token, now)?;
    if payload.get("typ")?.as_str()? != "access" {
        return None;
    }
    Some(payload.get("cid")?.as_str()?.to_owned())
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Where a client may be sent back to after the operator approves it.
///
/// AN ALLOW-LIST, NOT A PATTERN. An open redirect here hands an attacker the
/// authorization code: they start a flow, the operator approves what looks like
/// their own tool, and the code lands on the attacker's callback. Legacy carried
/// the same guard and its test proves it by registering a bad URI and expecting
/// refusal — a test that only registers the good one proves nothing.
fn redirect_is_allowed(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    match url.scheme() {
        // The hosted clients that speak remote MCP.
        "https" => {
            matches!(host, "claude.ai" | "claude.com")
                && url.path().starts_with("/api/mcp/auth_callback")
        }
        // A desktop or editor client listens on loopback. Plain http is correct
        // here and only here: the redirect never leaves the machine.
        "http" => matches!(host, "127.0.0.1" | "localhost" | "[::1]"),
        _ => false,
    }
}

/// A client id that carries the tool's name and proves this Hive issued it.
///
/// THE NAME HAS TO TRAVEL SOMEHOW. It is chosen at registration, and the
/// principal that needs it is created much later, when a token is first used —
/// with a deliberately stateless flow in between, so there is no row to look it
/// up in. Signing it into the id is what lets "Claude Desktop" appear on a board
/// write instead of a random string, without a clients table.
///
/// It also means a forged id cannot mint a principal: the id is checked before
/// anything is created.
fn sign_client_id(key: &[u8; 32], name: &str) -> Option<String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).ok()?;
    Some(sign(
        key,
        &json!({ "n": name, "r": Base64UrlUnpadded::encode_string(&bytes) }),
    ))
}

/// The registered name behind a client id, or None if this Hive did not issue it.
fn client_id_name(key: &[u8; 32], client_id: &str) -> Option<String> {
    unsign(key, client_id, 0)?
        .get("n")?
        .as_str()
        .map(str::to_owned)
}

/// What an outside tool should be called on the board, given the token it presented.
pub(crate) fn connection_name_for_token(state: &AppState, token: &str, now: i64) -> Option<String> {
    let key = signing_key(state)?;
    let client_id = client_for_token(state, token, now)?;
    client_id_name(&key, &client_id)
}

fn random_id() -> Option<String> {
    let mut bytes = [0u8; 18];
    getrandom::fill(&mut bytes).ok()?;
    Some(Base64UrlUnpadded::encode_string(&bytes))
}

#[derive(Deserialize)]
pub(super) struct RegisterRequest {
    #[serde(default)]
    redirect_uris: Vec<String>,
    #[serde(default)]
    client_name: Option<String>,
}

/// RFC 7591 Dynamic Client Registration.
pub(super) async fn register(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RegisterRequest>,
) -> Response {
    let Some(key) = signing_key(&state) else {
        return unavailable();
    };
    if request.redirect_uris.is_empty()
        || !request
            .redirect_uris
            .iter()
            .all(|uri| redirect_is_allowed(uri))
    {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "that redirect URI is not one this Hive will send an authorization code to",
        );
    }
    let name = request
        .client_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .unwrap_or("An outside tool");
    let Some(client_id) = sign_client_id(&key, name) else {
        return unavailable();
    };
    let secret = Base64UrlUnpadded::encode_string(&hmac_sha256(&key, client_id.as_bytes()));
    let _ = &headers;
    (
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "client_id": client_id,
            "client_secret": secret,
            "client_id_issued_at": crate::unix_timestamp(),
            "client_secret_expires_at": 0,
            "redirect_uris": request.redirect_uris,
            "client_name": name,
            "token_endpoint_auth_method": "client_secret_post",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "scope": SCOPE,
        })),
    )
        .into_response()
}

fn client_secret_matches(state: &AppState, client_id: &str, presented: &str) -> bool {
    let Some(key) = signing_key(state) else {
        return false;
    };
    let expected = Base64UrlUnpadded::encode_string(&hmac_sha256(&key, client_id.as_bytes()));
    presented.len() == expected.len() && bool::from(presented.as_bytes().ct_eq(expected.as_bytes()))
}

// ---------------------------------------------------------------------------
// Authorization codes
// ---------------------------------------------------------------------------

struct AuthorizationCode {
    client_id: String,
    redirect_uri: String,
    challenge: String,
    expires_at: i64,
}

/// IN MEMORY, AND SINGLE USE.
///
/// A code lives five minutes and is removed the moment it is redeemed, so a
/// replay finds nothing. Keeping them in memory means a restart drops codes
/// mid-flight; the client retries and the operator sees one extra approval. That
/// is a better trade than a table for rows whose whole life is measured in
/// minutes.
static AUTH_CODES: LazyLock<Mutex<HashMap<String, AuthorizationCode>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn issue_code(client_id: &str, redirect_uri: &str, challenge: &str, now: i64) -> Option<String> {
    let code = random_id()?;
    let mut codes = AUTH_CODES.lock().ok()?;
    codes.retain(|_, entry| entry.expires_at > now);
    codes.insert(
        code.clone(),
        AuthorizationCode {
            client_id: client_id.to_owned(),
            redirect_uri: redirect_uri.to_owned(),
            challenge: challenge.to_owned(),
            expires_at: now + AUTH_CODE_TTL,
        },
    );
    Some(code)
}

fn consume_code(code: &str, now: i64) -> Option<AuthorizationCode> {
    let mut codes = AUTH_CODES.lock().ok()?;
    codes.retain(|_, entry| entry.expires_at > now);
    codes.remove(code)
}

/// S256 only. `plain` is accepted by the RFC and defeats the point.
fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    let mut digest = Sha256::new();
    digest.update(verifier.as_bytes());
    let expected = Base64UrlUnpadded::encode_string(&digest.finalize());
    expected.len() == challenge.len() && bool::from(expected.as_bytes().ct_eq(challenge.as_bytes()))
}

// ---------------------------------------------------------------------------
// Authorize
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    response_type: Option<String>,
}

/// The operator's approval step.
///
/// GATED ON AN OPERATOR SESSION, through the same `authorize` every other
/// operator route uses. Without that gate anyone who learns the URL could
/// approve a client for themselves, which is the whole authorization server
/// reduced to a formality.
pub(super) async fn authorize_client(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<AuthorizeQuery>,
) -> Response {
    if query.response_type.as_deref().unwrap_or("code") != "code" {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_response_type",
            "this Hive issues authorization codes only",
        );
    }
    // Checked BEFORE the session gate, so a bad redirect can never be handed a
    // code even if the operator is signed in and clicks through.
    if !redirect_is_allowed(&query.redirect_uri) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "that redirect URI is not one this Hive will send an authorization code to",
        );
    }
    let (Some(challenge), Some(method)) = (
        query.code_challenge.as_deref(),
        query.code_challenge_method.as_deref(),
    ) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "this Hive requires PKCE: send code_challenge with code_challenge_method=S256",
        );
    };
    if method != "S256" {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "this Hive requires code_challenge_method=S256",
        );
    }
    if crate::auth::authorize(&state, &headers).is_err() {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "access_denied",
            "sign in to this Hive in the browser, then connect the tool again",
        );
    }
    let now = crate::unix_timestamp();
    let Some(code) = issue_code(&query.client_id, &query.redirect_uri, challenge, now) else {
        return unavailable();
    };
    let mut location = format!(
        "{}{}code={}",
        query.redirect_uri,
        if query.redirect_uri.contains('?') {
            '&'
        } else {
            '?'
        },
        code
    );
    if let Some(value) = query.state.as_deref() {
        location.push_str("&state=");
        location.push_str(value);
    }
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, location.as_str()),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(super) struct TokenRequest {
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
}

pub(super) async fn token(
    State(state): State<Arc<AppState>>,
    Form(request): Form<TokenRequest>,
) -> Response {
    let Some(key) = signing_key(&state) else {
        return unavailable();
    };
    let now = crate::unix_timestamp();
    match request.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(verifier)) =
                (request.code.as_deref(), request.code_verifier.as_deref())
            else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "an authorization code and its PKCE verifier are both required",
                );
            };
            // Consumed whatever happens next, so a failed exchange cannot be
            // retried against the same code.
            let Some(entry) = consume_code(code, now) else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "that authorization code is expired, already used, or unknown",
                );
            };
            if !pkce_matches(verifier, &entry.challenge) {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "that PKCE verifier does not match the challenge this code was issued for",
                );
            }
            if request.redirect_uri.as_deref() != Some(entry.redirect_uri.as_str()) {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "that redirect URI does not match the one this code was issued for",
                );
            }
            issued(&key, &entry.client_id, now)
        }
        "refresh_token" => {
            let Some(presented) = request.refresh_token.as_deref() else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "a refresh token is required",
                );
            };
            let Some(payload) = unsign(&key, presented, now) else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "that refresh token is expired or not one this Hive issued",
                );
            };
            if payload.get("typ").and_then(Value::as_str) != Some("refresh") {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "that is not a refresh token",
                );
            }
            let Some(client_id) = payload.get("cid").and_then(Value::as_str) else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "that refresh token names no client",
                );
            };
            // A confidential client must still prove it is the client.
            if request
                .client_secret
                .as_deref()
                .is_some_and(|secret| !client_secret_matches(&state, client_id, secret))
            {
                return oauth_error(
                    StatusCode::UNAUTHORIZED,
                    "invalid_client",
                    "that client secret does not match",
                );
            }
            let _ = request.client_id;
            issued(&key, client_id, now)
        }
        _ => oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "this Hive supports authorization_code and refresh_token",
        ),
    }
}

fn issued(key: &[u8; 32], client_id: &str, now: i64) -> Response {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "access_token": mint(key, "access", client_id, ACCESS_TOKEN_TTL, now),
            "token_type": "Bearer",
            "expires_in": ACCESS_TOKEN_TTL,
            "refresh_token": mint(key, "refresh", client_id, REFRESH_TOKEN_TTL, now),
            "scope": SCOPE,
        })),
    )
        .into_response()
}

fn oauth_error(status: StatusCode, code: &'static str, description: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "error": code, "error_description": description })),
    )
        .into_response()
}

/// Minting a token the way the token endpoint does, for tests in other modules.
///
/// Exposed rather than duplicated: a test that hand-rolls a token stops
/// testing the thing that issues them the moment either drifts.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{ACCESS_TOKEN_TTL, AppState, mint, sign_client_id, signing_key};

    pub(crate) fn issue_access_token(state: &AppState, client_name: &str) -> Option<String> {
        let key = signing_key(state)?;
        let client_id = sign_client_id(&key, client_name)?;
        // Minted against the real clock, because the caller verifies against it
        // too. Minting at zero produced a token that was already an hour stale
        // and failed as "unauthorised" rather than "expired".
        Some(mint(
            &key,
            "access",
            &client_id,
            ACCESS_TOKEN_TTL,
            crate::unix_timestamp(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        let store = TaskStore::in_memory().unwrap();
        router(
            crate::AppState::default()
                .with_task_store(store)
                .with_public_base_url("https://swarm.example.test")
                .unwrap(),
        )
    }

    async fn get(uri: &str) -> axum::response::Response {
        app()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Criterion 1. Discovery answers BEFORE any credential exists, because it
    /// is what a client reads to find out how to get one.
    #[tokio::test]
    async fn discovery_answers_without_a_credential() {
        let resource = get(PROTECTED_RESOURCE_PATH).await;
        assert_eq!(resource.status(), StatusCode::OK);
        let body = body_json(resource).await;
        assert_eq!(body["resource"], "https://swarm.example.test/mcp");
        assert_eq!(
            body["authorization_servers"][0],
            "https://swarm.example.test"
        );

        let server = get(AUTHORIZATION_SERVER_PATH).await;
        assert_eq!(server.status(), StatusCode::OK);
        let body = body_json(server).await;
        assert_eq!(body["issuer"], "https://swarm.example.test");
        assert_eq!(
            body["authorization_endpoint"],
            "https://swarm.example.test/oauth/authorize"
        );
        assert_eq!(
            body["token_endpoint"],
            "https://swarm.example.test/oauth/token"
        );
        assert_eq!(
            body["registration_endpoint"],
            "https://swarm.example.test/oauth/register"
        );
    }

    /// PKCE is advertised as the only method. A public client without it is
    /// interceptable, and every client that reaches this server is public.
    #[tokio::test]
    async fn only_pkce_is_offered() {
        let body = body_json(get(AUTHORIZATION_SERVER_PATH).await).await;
        assert_eq!(body["code_challenge_methods_supported"][0], "S256");
        assert_eq!(
            body["code_challenge_methods_supported"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    /// Criterion 1, second half. An ABSENT well-known must be 404, never 401 —
    /// otherwise the client's probe cannot tell "no discovery here" from
    /// "authenticate to learn how to authenticate".
    #[tokio::test]
    async fn an_absent_well_known_is_404_and_never_401() {
        let response = get("/.well-known/openid-configuration").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// Criterion 2. The refusal has to say where to go.
    ///
    /// ABLATION: drop the parameter from `challenge` and this fails — that is
    /// the whole difference between a 401 that is a dead end and one that is an
    /// invitation.
    #[test]
    fn the_challenge_names_where_to_authenticate() {
        let challenge = challenge(Some("https://swarm.example.test"));
        assert_eq!(
            challenge,
            r#"Bearer resource_metadata="https://swarm.example.test/.well-known/oauth-protected-resource""#
        );
        assert!(challenge.contains("resource_metadata="));
    }

    /// And it stays a valid challenge when there is nothing to point at, rather
    /// than advertising an origin nobody can reach.
    #[test]
    fn a_hive_with_no_known_address_still_refuses_honestly() {
        assert_eq!(challenge(None), "Bearer");
    }

    /// The configured public URL wins over the request's Host, because that is
    /// the only value that is right when the request arrives through a proxy.
    #[test]
    fn the_configured_public_url_wins_over_the_host_header() {
        let state = crate::AppState::default()
            .with_public_base_url("https://tunnel.example.test")
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:8766".parse().unwrap());
        assert_eq!(
            base_url(&state, &headers).as_deref(),
            Some("https://tunnel.example.test")
        );
    }

    /// With no public URL the Host keeps a local client working, which is how a
    /// developer connects one without configuring a tunnel first.
    #[test]
    fn a_loopback_host_is_enough_to_connect_locally() {
        let state = crate::AppState::default();
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:8766".parse().unwrap());
        assert_eq!(
            base_url(&state, &headers).as_deref(),
            Some("http://127.0.0.1:8766")
        );
    }

    /// A forwarded request carries its real scheme; using it stops discovery
    /// advertising http:// for a client that arrived over TLS.
    #[test]
    fn a_forwarded_scheme_is_honoured() {
        let state = crate::AppState::default();
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "swarm.example.test".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(
            base_url(&state, &headers).as_deref(),
            Some("https://swarm.example.test")
        );
    }

    const TOKEN: &str = "operator-token-for-tests";

    fn state_with_token(token: &str) -> crate::AppState {
        crate::AppState::default().with_terminal_host(
            swarm_terminal::HostClient::new("/unreachable/terminal.sock"),
            token,
        )
    }

    fn signed_app() -> axum::Router {
        router(
            state_with_token(TOKEN)
                .with_task_store(TaskStore::in_memory().unwrap())
                .with_public_base_url("https://swarm.example.test")
                .unwrap(),
        )
    }

    fn verifier_and_challenge() -> (String, String) {
        let verifier = "a".repeat(64);
        let mut digest = Sha256::new();
        digest.update(verifier.as_bytes());
        let challenge = Base64UrlUnpadded::encode_string(&digest.finalize());
        (verifier, challenge)
    }

    async fn register_a_client(app: &axum::Router, redirect: &str) -> Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/oauth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "redirect_uris": [redirect], "client_name": "A tool" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        body_json(response).await
    }

    /// Criterion 3, the whole point: register -> authorize -> token issues a
    /// working credential, and the endpoints the discovery documents promise
    /// actually answer.
    #[tokio::test]
    async fn a_client_can_register_be_approved_and_get_a_token() {
        let app = signed_app();
        let redirect = "https://claude.ai/api/mcp/auth_callback";
        let client = register_a_client(&app, redirect).await;
        let client_id = client["client_id"].as_str().unwrap().to_owned();
        let (verifier, challenge) = verifier_and_challenge();

        // The operator approves, with their session.
        let authorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/oauth/authorize?client_id={client_id}&redirect_uri={redirect}\
                         &response_type=code&code_challenge={challenge}&code_challenge_method=S256&state=xyz"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::FOUND);
        let location = authorized.headers()[header::LOCATION].to_str().unwrap();
        assert!(location.starts_with(redirect), "{location}");
        assert!(location.contains("state=xyz"), "{location}");
        let code = location
            .split("code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_owned();

        let issued = app
            .clone()
            .oneshot(token_request(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", &verifier),
                ("redirect_uri", redirect),
            ]))
            .await
            .unwrap();
        assert_eq!(issued.status(), StatusCode::OK);
        let issued = body_json(issued).await;
        assert_eq!(issued["token_type"], "Bearer");
        assert_eq!(issued["scope"], SCOPE);
        assert!(issued["access_token"].as_str().is_some());
        assert!(issued["refresh_token"].as_str().is_some());
    }

    fn token_request(fields: &[(&str, &str)]) -> Request<Body> {
        let body = fields
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");
        Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap()
    }

    fn urlencode(value: &str) -> String {
        value
            .bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (byte as char).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect()
    }

    /// Criterion 5, and THIS IS THE ABLATION. A test that only registers the
    /// good redirect URI proves nothing — an open redirect here hands an
    /// attacker the authorization code.
    #[tokio::test]
    async fn the_redirect_allow_list_bites() {
        let app = signed_app();
        for good in [
            "https://claude.ai/api/mcp/auth_callback",
            "http://127.0.0.1:33418/callback",
        ] {
            assert!(redirect_is_allowed(good), "{good}");
        }
        for bad in [
            "https://evil.example/cb",
            "https://claude.ai.evil.example/api/mcp/auth_callback",
            "https://claude.ai/wrong/path",
            "http://example.com/cb",
            "javascript:alert(1)",
            "https://not-claude.ai/api/mcp/auth_callback",
        ] {
            assert!(!redirect_is_allowed(bad), "{bad}");
        }

        let refused = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/oauth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "redirect_uris": ["https://evil.example/cb"] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    }

    /// Criterion 6. Without the session gate anyone who learns the URL can
    /// approve a client for themselves.
    #[tokio::test]
    async fn authorize_without_an_operator_session_issues_no_code() {
        let app = signed_app();
        let redirect = "https://claude.ai/api/mcp/auth_callback";
        let client = register_a_client(&app, redirect).await;
        let client_id = client["client_id"].as_str().unwrap();
        let (_, challenge) = verifier_and_challenge();
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/oauth/authorize?client_id={client_id}&redirect_uri={redirect}\
                         &response_type=code&code_challenge={challenge}&code_challenge_method=S256"
                    ))
                    // Not loopback, and no operator credential.
                    .header(header::HOST, "swarm.example.test")
                    .header("x-forwarded-for", "203.0.113.9")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get(header::LOCATION).is_none());
    }

    /// PKCE is required, not offered — a public client without it is
    /// interceptable, and every client here is public.
    #[tokio::test]
    async fn authorize_refuses_a_request_without_pkce() {
        let app = signed_app();
        let redirect = "https://claude.ai/api/mcp/auth_callback";
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/oauth/authorize?client_id=anything&redirect_uri={redirect}&response_type=code"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Criterion 4. A wrong verifier is refused, and a code is single use.
    #[tokio::test]
    async fn pkce_is_checked_and_a_code_is_single_use() {
        let (verifier, challenge) = verifier_and_challenge();
        assert!(pkce_matches(&verifier, &challenge));
        assert!(!pkce_matches("a different verifier", &challenge));

        let now = 1_000;
        let code = issue_code(
            "client",
            "https://claude.ai/api/mcp/auth_callback",
            &challenge,
            now,
        )
        .unwrap();
        assert!(consume_code(&code, now + 1).is_some());
        // Replay finds nothing.
        assert!(consume_code(&code, now + 1).is_none());

        let expiring = issue_code(
            "client",
            "https://claude.ai/api/mcp/auth_callback",
            &challenge,
            now,
        )
        .unwrap();
        assert!(consume_code(&expiring, now + AUTH_CODE_TTL + 1).is_none());
    }

    /// A signature this Hive did not make is not a token, and neither is an
    /// expired one.
    #[test]
    fn a_forged_or_stale_token_is_refused() {
        let key = [7u8; 32];
        let other = [8u8; 32];
        let token = mint(&key, "access", "client", ACCESS_TOKEN_TTL, 1_000);
        assert!(unsign(&key, &token, 1_001).is_some());
        assert!(unsign(&other, &token, 1_001).is_none());
        assert!(unsign(&key, &token, 1_000 + ACCESS_TOKEN_TTL + 1).is_none());

        // A tampered payload invalidates the MAC.
        let (body, mac) = token.split_once('.').unwrap();
        let mut decoded = Base64UrlUnpadded::decode_vec(body).unwrap();
        decoded[0] ^= 0x20;
        let tampered = format!("{}.{mac}", Base64UrlUnpadded::encode_string(&decoded));
        assert!(unsign(&key, &tampered, 1_001).is_none());
    }

    /// A refresh token is not an access token, however valid its signature.
    #[test]
    fn a_refresh_token_does_not_open_the_resource() {
        let state = state_with_token(TOKEN);
        let key = signing_key(&state).unwrap();
        let refresh = mint(&key, "refresh", "client", REFRESH_TOKEN_TTL, 1_000);
        assert!(client_for_token(&state, &refresh, 1_001).is_none());
        let access = mint(&key, "access", "client", ACCESS_TOKEN_TTL, 1_000);
        assert_eq!(
            client_for_token(&state, &access, 1_001).as_deref(),
            Some("client")
        );
    }

    /// Rotating the operator token disconnects every outside tool, which is the
    /// single revoke-everything lever and stops a connection outliving the
    /// credential it was authorised under.
    #[test]
    fn rotating_the_operator_token_invalidates_every_issued_token() {
        let before = state_with_token(TOKEN);
        let key = signing_key(&before).unwrap();
        let access = mint(&key, "access", "client", ACCESS_TOKEN_TTL, 1_000);
        assert!(client_for_token(&before, &access, 1_001).is_some());

        let after = state_with_token("a-rotated-operator-token");
        assert!(client_for_token(&after, &access, 1_001).is_none());
    }

    /// Knowing nothing is reported as such rather than guessed at. A discovery
    /// document advertising the wrong origin sends the client's browser
    /// somewhere that cannot complete the flow, and that surfaces as a broken
    /// redirect rather than as a missing setting.
    #[test]
    fn an_unknown_address_is_not_guessed() {
        let state = crate::AppState::default();
        assert_eq!(base_url(&state, &HeaderMap::new()), None);
    }
}
