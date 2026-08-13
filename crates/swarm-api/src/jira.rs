use std::{sync::Arc, time::Duration};

use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use swarm_domain::JiraConnectionState;

#[derive(Clone)]
pub(crate) enum JiraReadinessProbe {
    NotConfigured,
    Configured {
        client: Client,
        myself_url: Url,
        email: Arc<str>,
        api_token: Arc<str>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct JiraReadiness {
    pub configured: bool,
    pub connection: JiraConnectionState,
    pub account_name: Option<String>,
}

#[derive(Deserialize)]
struct JiraAccount {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

impl Default for JiraReadinessProbe {
    fn default() -> Self {
        Self::NotConfigured
    }
}

impl JiraReadinessProbe {
    pub(crate) fn configured(
        base_url: &str,
        email: impl Into<Arc<str>>,
        api_token: impl Into<Arc<str>>,
    ) -> Result<Self, String> {
        let base_url =
            Url::parse(base_url).map_err(|_| "SWARM_JIRA_BASE_URL must be a valid URL")?;
        let permitted_transport = base_url.scheme() == "https"
            || (base_url.scheme() == "http"
                && base_url
                    .host_str()
                    .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost")));
        if !permitted_transport || base_url.username() != "" || base_url.password().is_some() {
            return Err(
                "SWARM_JIRA_BASE_URL must use HTTPS and must not contain credentials".to_owned(),
            );
        }
        let myself_url = Url::parse(&format!(
            "{}/rest/api/3/myself",
            base_url.as_str().trim_end_matches('/')
        ))
        .map_err(|_| "SWARM_JIRA_BASE_URL could not form the Jira API URL")?;
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| format!("Jira HTTP client could not start: {error}"))?;
        Ok(Self::Configured {
            client,
            myself_url,
            email: email.into(),
            api_token: api_token.into(),
        })
    }

    pub(crate) async fn readiness(&self) -> JiraReadiness {
        let Self::Configured {
            client,
            myself_url,
            email,
            api_token,
        } = self
        else {
            return JiraReadiness {
                configured: false,
                connection: JiraConnectionState::NotConnected,
                account_name: None,
            };
        };
        let response = client
            .get(myself_url.clone())
            .basic_auth(email.as_ref(), Some(api_token.as_ref()))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await;
        let Ok(response) = response else {
            return unavailable();
        };
        match response.status() {
            status if status.is_success() => {
                let account_name = response
                    .bytes()
                    .await
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<JiraAccount>(&bytes).ok())
                    .and_then(|account| account.display_name)
                    .filter(|name| !name.trim().is_empty());
                JiraReadiness {
                    configured: true,
                    connection: JiraConnectionState::Ready,
                    account_name,
                }
            }
            StatusCode::UNAUTHORIZED => JiraReadiness {
                configured: true,
                connection: JiraConnectionState::CredentialsInvalid,
                account_name: None,
            },
            StatusCode::FORBIDDEN => JiraReadiness {
                configured: true,
                connection: JiraConnectionState::PermissionDenied,
                account_name: None,
            },
            _ => unavailable(),
        }
    }
}

fn unavailable() -> JiraReadiness {
    JiraReadiness {
        configured: true,
        connection: JiraConnectionState::NetworkUnavailable,
        account_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, http::StatusCode as AxumStatus, routing::get};
    use serde_json::json;

    async fn probe(response_status: AxumStatus) -> JiraReadiness {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/rest/api/3/myself",
            get(move || async move { (response_status, Json(json!({ "displayName": "Bea" }))) }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap()
        .readiness()
        .await
    }

    #[tokio::test]
    async fn distinguishes_ready_credentials_and_permission_states() {
        assert_eq!(
            probe(AxumStatus::OK).await.connection,
            JiraConnectionState::Ready
        );
        assert_eq!(
            probe(AxumStatus::UNAUTHORIZED).await.connection,
            JiraConnectionState::CredentialsInvalid
        );
        assert_eq!(
            probe(AxumStatus::FORBIDDEN).await.connection,
            JiraConnectionState::PermissionDenied
        );
    }

    #[tokio::test]
    async fn rejects_insecure_remote_and_embedded_credentials() {
        assert!(JiraReadinessProbe::configured("http://jira.example.test", "a", "b").is_err());
        assert!(JiraReadinessProbe::configured("https://a:b@jira.example.test", "a", "b").is_err());
    }
}
