use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use getrandom::fill as random_fill;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const AUTHORIZE_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
const SCOPES: &str = "read:jira-work write:jira-work read:jira-user offline_access";
const MAX_PENDING_STATES: usize = 8;
const STATE_LIFETIME_SECONDS: u64 = 600;

#[derive(Clone)]
pub(crate) struct JiraOAuthClient {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client,
    client_id: Arc<str>,
    client_secret: Arc<str>,
    redirect_uri: Url,
    authorize_url: Url,
    token_url: Url,
    resources_url: Url,
    token_path: PathBuf,
    tokens: Mutex<OAuthTokens>,
    pending: Mutex<VecDeque<PendingState>>,
}

#[derive(Clone, Debug)]
pub(crate) struct OAuthAccess {
    pub client: Client,
    pub base_url: Url,
    pub access_token: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct OAuthTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: f64,
    #[serde(default)]
    cloud_id: String,
    #[serde(default)]
    site_url: String,
    #[serde(default)]
    account_id: String,
}

#[derive(Clone, Debug)]
struct PendingState {
    value: String,
    created_at: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct AccessibleResource {
    id: String,
    url: String,
    #[serde(default)]
    scopes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OAuthError {
    NotConnected,
    CredentialsInvalid,
    PermissionDenied,
    NetworkUnavailable,
    InvalidResponse,
    InvalidState,
    Storage,
}

impl JiraOAuthClient {
    pub(crate) fn new(
        client: Client,
        client_id: impl Into<Arc<str>>,
        client_secret: impl Into<Arc<str>>,
        public_base_url: &str,
        token_path: PathBuf,
    ) -> Result<Self, String> {
        Self::new_with_endpoints(
            client,
            client_id,
            client_secret,
            public_base_url,
            token_path,
            AUTHORIZE_URL,
            TOKEN_URL,
            RESOURCES_URL,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_endpoints(
        client: Client,
        client_id: impl Into<Arc<str>>,
        client_secret: impl Into<Arc<str>>,
        public_base_url: &str,
        token_path: PathBuf,
        authorize_url: &str,
        token_url: &str,
        resources_url: &str,
    ) -> Result<Self, String> {
        let public_base_url =
            Url::parse(public_base_url).map_err(|_| "SWARM_PUBLIC_BASE_URL must be a valid URL")?;
        if public_base_url.scheme() != "https"
            || public_base_url.username() != ""
            || public_base_url.password().is_some()
        {
            return Err("SWARM_PUBLIC_BASE_URL must be an HTTPS URL without credentials".into());
        }
        let redirect_uri = public_base_url
            .join("auth/jira/callback")
            .map_err(|_| "SWARM_PUBLIC_BASE_URL could not form the Jira callback URL")?;
        let tokens = load_tokens(&token_path)?;
        Ok(Self {
            inner: Arc::new(Inner {
                client,
                client_id: client_id.into(),
                client_secret: client_secret.into(),
                redirect_uri,
                authorize_url: Url::parse(authorize_url)
                    .map_err(|_| "invalid Jira authorize URL")?,
                token_url: Url::parse(token_url).map_err(|_| "invalid Jira token URL")?,
                resources_url: Url::parse(resources_url)
                    .map_err(|_| "invalid Jira resources URL")?,
                token_path,
                tokens: Mutex::new(tokens),
                pending: Mutex::new(VecDeque::new()),
            }),
        })
    }

    pub(crate) async fn authorization_url(&self) -> Result<Url, OAuthError> {
        let mut bytes = [0_u8; 32];
        random_fill(&mut bytes).map_err(|_| OAuthError::Storage)?;
        let state = hex(&bytes);
        let now = unix_seconds();
        let mut pending = self.inner.pending.lock().await;
        pending.retain(|item| now.saturating_sub(item.created_at) <= STATE_LIFETIME_SECONDS);
        while pending.len() >= MAX_PENDING_STATES {
            pending.pop_front();
        }
        pending.push_back(PendingState {
            value: state.clone(),
            created_at: now,
        });
        drop(pending);

        let mut url = self.inner.authorize_url.clone();
        url.query_pairs_mut()
            .append_pair("audience", "api.atlassian.com")
            .append_pair("client_id", self.inner.client_id.as_ref())
            .append_pair("scope", SCOPES)
            .append_pair("redirect_uri", self.inner.redirect_uri.as_str())
            .append_pair("state", &state)
            .append_pair("response_type", "code")
            .append_pair("prompt", "consent");
        Ok(url)
    }

    pub(crate) async fn exchange_code(&self, state: &str, code: &str) -> Result<(), OAuthError> {
        if state.len() != 64 || code.is_empty() || code.len() > 4096 {
            return Err(OAuthError::InvalidState);
        }
        let now = unix_seconds();
        let mut pending = self.inner.pending.lock().await;
        let Some(position) = pending.iter().position(|item| {
            item.value == state && now.saturating_sub(item.created_at) <= STATE_LIFETIME_SECONDS
        }) else {
            return Err(OAuthError::InvalidState);
        };
        pending.remove(position);
        drop(pending);

        let response = self
            .inner
            .client
            .post(self.inner.token_url.clone())
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": self.inner.client_id.as_ref(),
                "client_secret": self.inner.client_secret.as_ref(),
                "code": code,
                "redirect_uri": self.inner.redirect_uri.as_str(),
            }))
            .send()
            .await
            .map_err(|_| OAuthError::NetworkUnavailable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(OAuthError::CredentialsInvalid);
        }
        if !response.status().is_success() {
            return Err(OAuthError::PermissionDenied);
        }
        let token = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| OAuthError::InvalidResponse)?;
        if token.access_token.is_empty() || token.refresh_token.as_deref().unwrap_or("").is_empty()
        {
            return Err(OAuthError::InvalidResponse);
        }
        let resource = discover_resource(
            &self.inner.client,
            self.inner.resources_url.clone(),
            &token.access_token,
        )
        .await?;
        let mut tokens = self.inner.tokens.lock().await;
        tokens.access_token = Some(token.access_token);
        tokens.refresh_token = token.refresh_token;
        tokens.expires_at = expiry_after(token.expires_in);
        tokens.cloud_id = resource.id;
        tokens.site_url = resource.url;
        save_tokens(&self.inner.token_path, &tokens)?;
        Ok(())
    }

    pub(crate) async fn access(&self) -> Result<OAuthAccess, OAuthError> {
        let mut tokens = self.inner.tokens.lock().await;
        if tokens.cloud_id.is_empty() || tokens.refresh_token.as_deref().unwrap_or("").is_empty() {
            return Err(OAuthError::NotConnected);
        }
        if tokens.expires_at <= expiry_after(60) {
            refresh(&self.inner, &mut tokens).await?;
        }
        let access_token = tokens
            .access_token
            .clone()
            .filter(|value| !value.is_empty())
            .ok_or(OAuthError::NotConnected)?;
        let base_url = Url::parse(&format!(
            "https://api.atlassian.com/ex/jira/{}",
            tokens.cloud_id
        ))
        .map_err(|_| OAuthError::InvalidResponse)?;
        Ok(OAuthAccess {
            client: self.inner.client.clone(),
            base_url,
            access_token,
        })
    }

    pub(crate) async fn site_url(&self) -> Option<Url> {
        let tokens = self.inner.tokens.lock().await;
        let url = Url::parse(tokens.site_url.trim()).ok()?;
        (url.scheme() == "https" && url.username().is_empty() && url.password().is_none())
            .then_some(url)
    }

    pub(crate) async fn disconnect(&self) -> Result<(), OAuthError> {
        *self.inner.tokens.lock().await = OAuthTokens::default();
        match fs::remove_file(&self.inner.token_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(OAuthError::Storage),
        }
    }
}

async fn refresh(inner: &Inner, tokens: &mut OAuthTokens) -> Result<(), OAuthError> {
    let refresh_token = tokens
        .refresh_token
        .clone()
        .filter(|value| !value.is_empty())
        .ok_or(OAuthError::NotConnected)?;
    let response = inner
        .client
        .post(inner.token_url.clone())
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": inner.client_id.as_ref(),
            "client_secret": inner.client_secret.as_ref(),
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|_| OAuthError::NetworkUnavailable)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(OAuthError::CredentialsInvalid);
    }
    if !response.status().is_success() {
        return Err(OAuthError::NetworkUnavailable);
    }
    let next = response
        .json::<TokenResponse>()
        .await
        .map_err(|_| OAuthError::InvalidResponse)?;
    if next.access_token.is_empty() {
        return Err(OAuthError::InvalidResponse);
    }
    tokens.access_token = Some(next.access_token);
    if let Some(next_refresh) = next.refresh_token.filter(|value| !value.is_empty()) {
        tokens.refresh_token = Some(next_refresh);
    }
    tokens.expires_at = expiry_after(next.expires_in);
    save_tokens(&inner.token_path, tokens)
}

async fn discover_resource(
    client: &Client,
    resources_url: Url,
    token: &str,
) -> Result<AccessibleResource, OAuthError> {
    let response = client
        .get(resources_url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| OAuthError::NetworkUnavailable)?;
    if !response.status().is_success() {
        return Err(OAuthError::PermissionDenied);
    }
    let resources = response
        .json::<Vec<AccessibleResource>>()
        .await
        .map_err(|_| OAuthError::InvalidResponse)?;
    let required = ["read:jira-work", "read:jira-user"];
    let mut eligible = resources.into_iter().filter(|resource| {
        !resource.id.is_empty()
            && Url::parse(&resource.url).is_ok_and(|url| url.scheme() == "https")
            && required
                .iter()
                .all(|scope| resource.scopes.iter().any(|granted| granted == scope))
    });
    let resource = eligible.next().ok_or(OAuthError::PermissionDenied)?;
    if eligible.next().is_some() {
        return Err(OAuthError::InvalidResponse);
    }
    Ok(resource)
}

fn load_tokens(path: &Path) -> Result<OAuthTokens, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Jira OAuth token file is invalid JSON".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(OAuthTokens::default()),
        Err(error) => Err(format!("Jira OAuth token file could not be read: {error}")),
    }
}

fn save_tokens(path: &Path, tokens: &OAuthTokens) -> Result<(), OAuthError> {
    let parent = path.parent().ok_or(OAuthError::Storage)?;
    fs::create_dir_all(parent).map_err(|_| OAuthError::Storage)?;
    secure_directory(parent)?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(tokens).map_err(|_| OAuthError::Storage)?;
    write_private(&temporary, &bytes)?;
    fs::rename(temporary, path).map_err(|_| OAuthError::Storage)
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), OAuthError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| OAuthError::Storage)
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> Result<(), OAuthError> {
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), OAuthError> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| OAuthError::Storage)?;
    file.write_all(bytes).map_err(|_| OAuthError::Storage)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), OAuthError> {
    fs::write(path, bytes).map_err(|_| OAuthError::Storage)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

#[allow(clippy::cast_precision_loss)]
fn expiry_after(seconds: u64) -> f64 {
    unix_seconds() as f64 + seconds as f64
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    #[tokio::test]
    async fn authorization_is_state_bound_rotates_refresh_tokens_and_survives_restart() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/token",
                post(|Json(body): Json<Value>| async move {
                    if body["grant_type"] == "refresh_token" {
                        Json(json!({
                            "access_token": "access-2",
                            "refresh_token": "refresh-2",
                            "expires_in": 3600
                        }))
                    } else {
                        Json(json!({
                            "access_token": "access-1",
                            "refresh_token": "refresh-1",
                            "expires_in": 0
                        }))
                    }
                }),
            )
            .route(
                "/resources",
                get(|| async {
                    Json(json!([{
                        "id": "cloud-1",
                        "url": "https://example.atlassian.net",
                        "scopes": ["read:jira-work", "write:jira-work", "read:jira-user"]
                    }]))
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let token_path = directory.path().join("secrets/jira-oauth.json");
        let token_url = format!("http://{address}/token");
        let resources_url = format!("http://{address}/resources");
        let client = JiraOAuthClient::new_with_endpoints(
            Client::new(),
            "client-id",
            "client-secret",
            "https://swarm.example.test/",
            token_path.clone(),
            "https://auth.atlassian.test/authorize",
            &token_url,
            &resources_url,
        )
        .unwrap();
        let authorization = client.authorization_url().await.unwrap();
        let state = authorization
            .query_pairs()
            .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
            .unwrap();
        assert_eq!(
            authorization
                .query_pairs()
                .find_map(|(key, value)| (key == "redirect_uri").then(|| value.into_owned()))
                .unwrap(),
            "https://swarm.example.test/auth/jira/callback"
        );
        client.exchange_code(&state, "one-time-code").await.unwrap();
        assert_eq!(
            client.exchange_code(&state, "replay").await,
            Err(OAuthError::InvalidState)
        );
        let access = client.access().await.unwrap();
        assert_eq!(access.access_token, "access-2");
        assert_eq!(
            access.base_url.as_str(),
            "https://api.atlassian.com/ex/jira/cloud-1"
        );
        let stored: OAuthTokens = serde_json::from_slice(&fs::read(&token_path).unwrap()).unwrap();
        assert_eq!(stored.refresh_token.as_deref(), Some("refresh-2"));

        let restarted = JiraOAuthClient::new_with_endpoints(
            Client::new(),
            "client-id",
            "client-secret",
            "https://swarm.example.test/",
            token_path.clone(),
            "https://auth.atlassian.test/authorize",
            &token_url,
            &resources_url,
        )
        .unwrap();
        assert_eq!(restarted.access().await.unwrap().access_token, "access-2");
        restarted.disconnect().await.unwrap();
        assert!(!token_path.exists());
    }

    #[test]
    fn public_callback_must_be_https_and_secret_free() {
        for invalid in [
            "http://swarm.example.test",
            "https://user:secret@swarm.example.test",
        ] {
            assert!(
                JiraOAuthClient::new(
                    Client::new(),
                    "id",
                    "secret",
                    invalid,
                    PathBuf::from("tokens.json")
                )
                .is_err()
            );
        }
    }
}
