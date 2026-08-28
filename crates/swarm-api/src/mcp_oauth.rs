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
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::Arc;

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
