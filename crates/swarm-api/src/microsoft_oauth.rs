use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64ct::{Base64UrlUnpadded, Encoding};
use getrandom::fill as random_fill;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

const GRAPH_BASE_URL: &str = "https://graph.microsoft.com/v1.0/";
const SCOPES: &str = "openid profile offline_access User.Read Mail.Read Mail.Send";
const MAX_PENDING_STATES: usize = 8;
const STATE_LIFETIME_SECONDS: u64 = 600;

#[derive(Clone)]
pub(crate) struct MicrosoftOAuthClient {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client,
    client_id: Arc<str>,
    client_secret: Arc<str>,
    redirect_uri: Url,
    authorize_url: Url,
    token_url: Url,
    graph_base_url: Url,
    token_path: PathBuf,
    tokens: Mutex<OAuthTokens>,
    pending: Mutex<VecDeque<PendingState>>,
}

#[derive(Clone, Debug)]
pub(crate) struct MicrosoftAccess {
    pub client: Client,
    pub base_url: Url,
    pub access_token: String,
    pub integration_id: String,
    pub account_name: String,
    pub account_address: String,
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
    account_id: String,
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    account_address: String,
}

#[derive(Clone, Debug)]
struct PendingState {
    value: String,
    verifier: String,
    created_at: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct GraphAccount {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    user_principal_name: String,
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

impl MicrosoftOAuthClient {
    pub(crate) fn new(
        client: Client,
        tenant_id: &str,
        client_id: impl Into<Arc<str>>,
        client_secret: impl Into<Arc<str>>,
        public_base_url: &str,
        token_path: PathBuf,
    ) -> Result<Self, String> {
        let tenant_id = tenant_id.trim();
        if tenant_id.is_empty()
            || tenant_id.len() > 128
            || !tenant_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("SWARM_EMAIL_TENANT_ID must be a tenant UUID or organizations".into());
        }
        let authority = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/");
        Self::new_with_endpoints(
            client,
            client_id,
            client_secret,
            public_base_url,
            token_path,
            &format!("{authority}authorize"),
            &format!("{authority}token"),
            GRAPH_BASE_URL,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_endpoints(
        client: Client,
        client_id: impl Into<Arc<str>>,
        client_secret: impl Into<Arc<str>>,
        public_base_url: &str,
        token_path: PathBuf,
        authorize_url: &str,
        token_url: &str,
        graph_base_url: &str,
    ) -> Result<Self, String> {
        let public_base_url =
            Url::parse(public_base_url).map_err(|_| "SWARM_PUBLIC_BASE_URL must be a valid URL")?;
        if public_base_url.scheme() != "https"
            || !public_base_url.username().is_empty()
            || public_base_url.password().is_some()
            || public_base_url.query().is_some()
            || public_base_url.fragment().is_some()
        {
            return Err(
                "SWARM_PUBLIC_BASE_URL must be an HTTPS URL without credentials, query, or fragment"
                    .into(),
            );
        }
        let redirect_uri = public_base_url
            .join("auth/email/callback")
            .map_err(|_| "SWARM_PUBLIC_BASE_URL could not form the email callback URL")?;
        let tokens = load_tokens(&token_path)?;
        Ok(Self {
            inner: Arc::new(Inner {
                client,
                client_id: client_id.into(),
                client_secret: client_secret.into(),
                redirect_uri,
                authorize_url: Url::parse(authorize_url)
                    .map_err(|_| "invalid Microsoft authorize URL")?,
                token_url: Url::parse(token_url).map_err(|_| "invalid Microsoft token URL")?,
                graph_base_url: Url::parse(graph_base_url)
                    .map_err(|_| "invalid Microsoft Graph URL")?,
                token_path,
                tokens: Mutex::new(tokens),
                pending: Mutex::new(VecDeque::new()),
            }),
        })
    }

    pub(crate) async fn authorization_url(&self) -> Result<Url, OAuthError> {
        let mut state_bytes = [0_u8; 32];
        let mut verifier_bytes = [0_u8; 48];
        random_fill(&mut state_bytes).map_err(|_| OAuthError::Storage)?;
        random_fill(&mut verifier_bytes).map_err(|_| OAuthError::Storage)?;
        let state = Base64UrlUnpadded::encode_string(&state_bytes);
        let verifier = Base64UrlUnpadded::encode_string(&verifier_bytes);
        let challenge = Base64UrlUnpadded::encode_string(&Sha256::digest(verifier.as_bytes()));
        let now = unix_seconds();
        let mut pending = self.inner.pending.lock().await;
        pending.retain(|item| now.saturating_sub(item.created_at) <= STATE_LIFETIME_SECONDS);
        while pending.len() >= MAX_PENDING_STATES {
            pending.pop_front();
        }
        pending.push_back(PendingState {
            value: state.clone(),
            verifier,
            created_at: now,
        });
        drop(pending);

        let mut url = self.inner.authorize_url.clone();
        url.query_pairs_mut()
            .append_pair("client_id", self.inner.client_id.as_ref())
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", self.inner.redirect_uri.as_str())
            .append_pair("response_mode", "query")
            .append_pair("scope", SCOPES)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("prompt", "select_account");
        Ok(url)
    }

    pub(crate) async fn exchange_code(&self, state: &str, code: &str) -> Result<(), OAuthError> {
        if state.len() < 40 || state.len() > 128 || code.is_empty() || code.len() > 4_096 {
            return Err(OAuthError::InvalidState);
        }
        let now = unix_seconds();
        let mut pending = self.inner.pending.lock().await;
        let Some(position) = pending.iter().position(|item| {
            item.value == state && now.saturating_sub(item.created_at) <= STATE_LIFETIME_SECONDS
        }) else {
            return Err(OAuthError::InvalidState);
        };
        let verifier = pending.remove(position).expect("position exists").verifier;
        drop(pending);

        let response = self
            .inner
            .client
            .post(self.inner.token_url.clone())
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form_body(&[
                ("client_id", self.inner.client_id.as_ref()),
                ("client_secret", self.inner.client_secret.as_ref()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.inner.redirect_uri.as_str()),
                ("scope", SCOPES),
                ("code_verifier", &verifier),
            ]))
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
        let account = discover_account(
            &self.inner.client,
            &self.inner.graph_base_url,
            &token.access_token,
        )
        .await?;
        let mut tokens = self.inner.tokens.lock().await;
        tokens.access_token = Some(token.access_token);
        tokens.refresh_token = token.refresh_token;
        tokens.expires_at = expiry_after(token.expires_in);
        tokens.account_id = account.id;
        tokens.account_name = account.display_name;
        tokens.account_address = account.mail.unwrap_or(account.user_principal_name);
        if tokens.account_id.is_empty() || tokens.account_address.is_empty() {
            return Err(OAuthError::InvalidResponse);
        }
        save_tokens(&self.inner.token_path, &tokens)
    }

    pub(crate) async fn access(&self) -> Result<MicrosoftAccess, OAuthError> {
        let mut tokens = self.inner.tokens.lock().await;
        if tokens.account_id.is_empty()
            || tokens.account_address.is_empty()
            || tokens.refresh_token.as_deref().unwrap_or("").is_empty()
        {
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
        Ok(MicrosoftAccess {
            client: self.inner.client.clone(),
            base_url: self.inner.graph_base_url.clone(),
            access_token,
            integration_id: tokens.account_id.clone(),
            account_name: tokens.account_name.clone(),
            account_address: tokens.account_address.clone(),
        })
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
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form_body(&[
            ("client_id", inner.client_id.as_ref()),
            ("client_secret", inner.client_secret.as_ref()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("scope", SCOPES),
        ]))
        .send()
        .await
        .map_err(|_| OAuthError::NetworkUnavailable)?;
    if matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
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

async fn discover_account(
    client: &Client,
    graph_base_url: &Url,
    token: &str,
) -> Result<GraphAccount, OAuthError> {
    let mut url = graph_base_url
        .join("me")
        .map_err(|_| OAuthError::InvalidResponse)?;
    url.query_pairs_mut()
        .append_pair("$select", "id,displayName,mail,userPrincipalName");
    let response = client
        .get(url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| OAuthError::NetworkUnavailable)?;
    if matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err(OAuthError::PermissionDenied);
    }
    if !response.status().is_success() {
        return Err(OAuthError::NetworkUnavailable);
    }
    response
        .json::<GraphAccount>()
        .await
        .map_err(|_| OAuthError::InvalidResponse)
}

fn load_tokens(path: &Path) -> Result<OAuthTokens, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Email OAuth token file is invalid JSON".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(OAuthTokens::default()),
        Err(error) => Err(format!("Email OAuth token file could not be read: {error}")),
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

fn form_body(fields: &[(&str, &str)]) -> String {
    let mut url = Url::parse("https://form.invalid/").expect("static form URL is valid");
    {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in fields {
            pairs.append_pair(name, value);
        }
    }
    url.query().unwrap_or_default().to_owned()
}

#[allow(clippy::cast_precision_loss)]
fn expiry_after(seconds: u64) -> f64 {
    unix_seconds() as f64 + seconds as f64
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn authorization_uses_pkce_and_rotating_private_tokens() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/token",
                post(|body: String| async move {
                    if body.contains("grant_type=refresh_token") {
                        Json(json!({"access_token":"access-2","refresh_token":"refresh-2","expires_in":3600}))
                    } else {
                        assert!(body.contains("code_verifier="));
                        Json(json!({"access_token":"access-1","refresh_token":"refresh-1","expires_in":0}))
                    }
                }),
            )
            .route(
                "/me",
                get(|| async {
                    Json(json!({
                        "id":"account-1",
                        "displayName":"Operator",
                        "mail":"operator@example.test",
                        "userPrincipalName":"operator@example.test"
                    }))
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let token_path = directory.path().join("secrets/email-oauth.json");
        let client = MicrosoftOAuthClient::new_with_endpoints(
            Client::new(),
            "client-id",
            "client-secret",
            "https://swarm.example.test/",
            token_path.clone(),
            "https://login.microsoft.test/authorize",
            &format!("http://{address}/token"),
            &format!("http://{address}/"),
        )
        .unwrap();
        let authorization = client.authorization_url().await.unwrap();
        let query = authorization
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        assert!(
            query
                .get("code_challenge")
                .is_some_and(|value| value.len() >= 43)
        );
        assert_eq!(
            query.get("redirect_uri").unwrap(),
            "https://swarm.example.test/auth/email/callback"
        );
        let state = query.get("state").unwrap().to_string();
        client.exchange_code(&state, "one-time-code").await.unwrap();
        assert_eq!(
            client.exchange_code(&state, "replay").await,
            Err(OAuthError::InvalidState)
        );
        let access = client.access().await.unwrap();
        assert_eq!(access.access_token, "access-2");
        assert_eq!(access.integration_id, "account-1");
        assert_eq!(access.account_address, "operator@example.test");
        assert!(token_path.exists());
        client.disconnect().await.unwrap();
        assert!(!token_path.exists());
    }

    #[test]
    fn callback_and_tenant_are_bounded() {
        for (tenant, public_url) in [
            ("tenant/escape", "https://swarm.example.test"),
            ("organizations", "http://swarm.example.test"),
            ("organizations", "https://user:secret@swarm.example.test"),
            (
                "organizations",
                "https://swarm.example.test?redirect=elsewhere",
            ),
            ("organizations", "https://swarm.example.test#fragment"),
        ] {
            assert!(
                MicrosoftOAuthClient::new(
                    Client::new(),
                    tenant,
                    "id",
                    "secret",
                    public_url,
                    PathBuf::from("tokens.json"),
                )
                .is_err()
            );
        }
    }
}
