use std::{collections::BTreeMap, sync::Arc, time::Duration};

use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use swarm_domain::{JiraConnectionState, TaskState};

use crate::jira_oauth::{JiraOAuthClient, OAuthError};

const PAGE_SIZE: usize = 50;
const MAX_PROJECTS: usize = 500;
const MAX_PROJECT_STATUSES: usize = 128;
const MAX_ISSUES: usize = 200;

#[derive(Clone, Default)]
pub(crate) enum JiraReadinessProbe {
    #[default]
    NotConfigured,
    Configured {
        client: Box<Client>,
        base_url: Url,
        email: Arc<str>,
        api_token: Arc<str>,
    },
    OAuth(JiraOAuthClient),
}

#[derive(Clone)]
struct JiraAccess {
    client: Client,
    base_url: Url,
    authorization: JiraAuthorization,
}

#[derive(Clone)]
enum JiraAuthorization {
    Basic {
        email: Arc<str>,
        api_token: Arc<str>,
    },
    Bearer(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct JiraReadiness {
    pub configured: bool,
    pub connection: JiraConnectionState,
    pub account_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct JiraProject {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct JiraProjectStatus {
    pub id: String,
    pub name: String,
    pub category_key: String,
    pub recommended_task_state: TaskState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct JiraIssue {
    pub id: String,
    pub key: String,
    pub summary: String,
    pub status_id: String,
    pub status_name: String,
    pub assignee_account_id: Option<String>,
    pub assignee_name: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JiraAdapterError {
    NotConfigured,
    CredentialsInvalid,
    PermissionDenied,
    NetworkUnavailable,
    InvalidResponse,
    ResponseLimitExceeded,
}

impl std::fmt::Display for JiraAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotConfigured => "Jira is not connected",
            Self::CredentialsInvalid => "Jira credentials are invalid",
            Self::PermissionDenied => "Jira denied access",
            Self::NetworkUnavailable => "Jira is temporarily unavailable",
            Self::InvalidResponse => "Jira returned an invalid response",
            Self::ResponseLimitExceeded => "Jira response exceeded the bounded operation limit",
        })
    }
}

#[derive(Deserialize)]
struct JiraAccount {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct JiraProjectPage {
    #[serde(default)]
    values: Vec<JiraProject>,
    #[serde(rename = "isLast")]
    is_last: Option<bool>,
    total: Option<usize>,
}

#[derive(Deserialize)]
struct JiraIssueTypeStatuses {
    #[serde(default)]
    statuses: Vec<JiraStatusResponse>,
}

#[derive(Deserialize)]
struct JiraStatusResponse {
    id: String,
    name: String,
    #[serde(rename = "statusCategory")]
    status_category: JiraStatusCategory,
}

#[derive(Deserialize)]
struct JiraStatusCategory {
    key: String,
}

#[derive(Deserialize)]
struct JiraIssuePage {
    #[serde(rename = "isLast")]
    is_last: Option<bool>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    #[serde(default)]
    issues: Vec<JiraIssueResponse>,
}

#[derive(Deserialize)]
struct JiraIssueResponse {
    id: String,
    key: String,
    fields: JiraIssueFields,
}

#[derive(Deserialize)]
struct JiraIssueFields {
    summary: String,
    status: JiraIssueStatus,
    assignee: Option<JiraIssueAssignee>,
    updated: String,
}

#[derive(Deserialize)]
struct JiraIssueStatus {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct JiraIssueAssignee {
    #[serde(rename = "accountId")]
    account_id: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

impl JiraReadinessProbe {
    pub(crate) fn oauth(client: JiraOAuthClient) -> Self {
        Self::OAuth(client)
    }

    pub(crate) fn oauth_client(&self) -> Option<&JiraOAuthClient> {
        if let Self::OAuth(client) = self {
            Some(client)
        } else {
            None
        }
    }

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
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| format!("Jira HTTP client could not start: {error}"))?;
        Ok(Self::Configured {
            client: Box::new(client),
            base_url,
            email: email.into(),
            api_token: api_token.into(),
        })
    }

    pub(crate) async fn readiness(&self) -> JiraReadiness {
        if matches!(self, Self::NotConfigured) {
            return JiraReadiness {
                configured: false,
                connection: JiraConnectionState::NotConnected,
                account_name: None,
            };
        }
        let access = match self.access().await {
            Ok(access) => access,
            Err(JiraAdapterError::NotConfigured) => {
                return JiraReadiness {
                    configured: true,
                    connection: JiraConnectionState::NotConnected,
                    account_name: None,
                };
            }
            Err(JiraAdapterError::CredentialsInvalid) => {
                return JiraReadiness {
                    configured: true,
                    connection: JiraConnectionState::CredentialsInvalid,
                    account_name: None,
                };
            }
            Err(JiraAdapterError::PermissionDenied) => {
                return JiraReadiness {
                    configured: true,
                    connection: JiraConnectionState::PermissionDenied,
                    account_name: None,
                };
            }
            Err(_) => return unavailable(),
        };
        let Ok(mut project_probe_url) = endpoint(&access.base_url, "/rest/api/3/project/search")
        else {
            return unavailable();
        };
        project_probe_url
            .query_pairs_mut()
            .append_pair("startAt", "0")
            .append_pair("maxResults", "1");
        let project_response =
            authorize(access.client.get(project_probe_url), &access.authorization)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await;
        let Ok(project_response) = project_response else {
            return unavailable();
        };
        match project_response.status() {
            StatusCode::UNAUTHORIZED => {
                return JiraReadiness {
                    configured: true,
                    connection: JiraConnectionState::CredentialsInvalid,
                    account_name: None,
                };
            }
            StatusCode::FORBIDDEN => {
                return JiraReadiness {
                    configured: true,
                    connection: JiraConnectionState::PermissionDenied,
                    account_name: None,
                };
            }
            status if status.is_success() => {}
            _ => return unavailable(),
        }

        // Project discovery is the capability Swarm requires. Profile access is
        // cosmetic and older otherwise-valid grants may not include read:jira-user.
        let account_name = if let Ok(myself_url) = endpoint(&access.base_url, "/rest/api/3/myself")
        {
            match authorize(access.client.get(myself_url), &access.authorization)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response
                    .bytes()
                    .await
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<JiraAccount>(&bytes).ok())
                    .and_then(|account| account.display_name)
                    .filter(|name| !name.trim().is_empty()),
                _ => None,
            }
        } else {
            None
        };
        JiraReadiness {
            configured: true,
            connection: JiraConnectionState::Ready,
            account_name,
        }
    }

    pub(crate) async fn projects(
        &self,
        query: Option<&str>,
    ) -> Result<Vec<JiraProject>, JiraAdapterError> {
        let access = self.access().await?;
        let mut projects = Vec::new();
        for start_at in (0..MAX_PROJECTS).step_by(PAGE_SIZE) {
            let mut url = endpoint(&access.base_url, "/rest/api/3/project/search")?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("startAt", &start_at.to_string());
                pairs.append_pair("maxResults", &PAGE_SIZE.to_string());
                if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
                    pairs.append_pair("query", query);
                }
            }
            let response = authorize(access.client.get(url), &access.authorization)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
            ensure_success(response.status())?;
            let bytes = response
                .bytes()
                .await
                .map_err(|_| JiraAdapterError::InvalidResponse)?;
            let page = serde_json::from_slice::<JiraProjectPage>(&bytes)
                .map_err(|_| JiraAdapterError::InvalidResponse)?;
            if page.values.len() > PAGE_SIZE {
                return Err(JiraAdapterError::ResponseLimitExceeded);
            }
            let page_len = page.values.len();
            projects.extend(page.values.into_iter().filter(valid_project));
            if projects.len() > MAX_PROJECTS {
                return Err(JiraAdapterError::ResponseLimitExceeded);
            }
            let exhausted = page.is_last == Some(true)
                || page_len < PAGE_SIZE
                || page.total.is_some_and(|total| start_at + page_len >= total);
            if exhausted {
                return Ok(projects);
            }
        }
        Err(JiraAdapterError::ResponseLimitExceeded)
    }

    pub(crate) async fn project_statuses(
        &self,
        project_id_or_key: &str,
    ) -> Result<Vec<JiraProjectStatus>, JiraAdapterError> {
        let access = self.access().await?;
        let project = project_id_or_key.trim();
        if project.is_empty() || project.len() > 128 || project.chars().any(char::is_control) {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let mut url = endpoint(&access.base_url, "/rest/api/3/project/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(project)
            .push("statuses");
        let response = authorize(access.client.get(url), &access.authorization)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
        ensure_success(response.status())?;
        let bytes = response
            .bytes()
            .await
            .map_err(|_| JiraAdapterError::InvalidResponse)?;
        let issue_types = serde_json::from_slice::<Vec<JiraIssueTypeStatuses>>(&bytes)
            .map_err(|_| JiraAdapterError::InvalidResponse)?;
        let mut statuses = BTreeMap::new();
        for status in issue_types.into_iter().flat_map(|item| item.statuses) {
            if statuses.len() >= MAX_PROJECT_STATUSES && !statuses.contains_key(&status.id) {
                return Err(JiraAdapterError::ResponseLimitExceeded);
            }
            if status.id.is_empty() || status.name.trim().is_empty() {
                continue;
            }
            let category_key = status.status_category.key;
            statuses.insert(
                status.id.clone(),
                JiraProjectStatus {
                    id: status.id,
                    name: status.name,
                    recommended_task_state: recommended_state(&category_key),
                    category_key,
                },
            );
        }
        Ok(statuses.into_values().collect())
    }

    pub(crate) async fn issues(
        &self,
        project_id: &str,
    ) -> Result<Vec<JiraIssue>, JiraAdapterError> {
        let access = self.access().await?;
        let project_id = project_id.trim();
        if project_id.is_empty()
            || project_id.len() > 128
            || project_id.chars().any(char::is_control)
        {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let jql_project = if project_id
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            project_id.to_owned()
        } else {
            format!(
                "\"{}\"",
                project_id.replace('\\', "\\\\").replace('\"', "\\\"")
            )
        };
        let jql = format!("project = {jql_project} ORDER BY updated DESC");
        let mut issues = Vec::new();
        let mut next_page_token: Option<String> = None;
        for _ in 0..(MAX_ISSUES / PAGE_SIZE) {
            let mut url = endpoint(&access.base_url, "/rest/api/3/search/jql")?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("jql", &jql);
                pairs.append_pair("maxResults", &PAGE_SIZE.to_string());
                pairs.append_pair("fields", "summary,status,assignee,updated");
                if let Some(token) = next_page_token.as_deref() {
                    pairs.append_pair("nextPageToken", token);
                }
            }
            let response = authorize(access.client.get(url), &access.authorization)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
            ensure_success(response.status())?;
            let bytes = response
                .bytes()
                .await
                .map_err(|_| JiraAdapterError::InvalidResponse)?;
            let page = serde_json::from_slice::<JiraIssuePage>(&bytes)
                .map_err(|_| JiraAdapterError::InvalidResponse)?;
            if page.issues.len() > PAGE_SIZE {
                return Err(JiraAdapterError::ResponseLimitExceeded);
            }
            issues.extend(page.issues.into_iter().filter_map(jira_issue));
            if issues.len() > MAX_ISSUES {
                return Err(JiraAdapterError::ResponseLimitExceeded);
            }
            if page.is_last == Some(true) || page.next_page_token.is_none() {
                return Ok(issues);
            }
            next_page_token = page.next_page_token;
        }
        Err(JiraAdapterError::ResponseLimitExceeded)
    }

    async fn access(&self) -> Result<JiraAccess, JiraAdapterError> {
        match self {
            Self::NotConfigured => Err(JiraAdapterError::NotConfigured),
            Self::Configured {
                client,
                base_url,
                email,
                api_token,
                ..
            } => Ok(JiraAccess {
                client: client.as_ref().clone(),
                base_url: base_url.clone(),
                authorization: JiraAuthorization::Basic {
                    email: email.clone(),
                    api_token: api_token.clone(),
                },
            }),
            Self::OAuth(oauth) => {
                let access = oauth.access().await.map_err(oauth_error)?;
                Ok(JiraAccess {
                    client: access.client,
                    base_url: access.base_url,
                    authorization: JiraAuthorization::Bearer(access.access_token),
                })
            }
        }
    }
}

fn authorize(
    request: reqwest::RequestBuilder,
    authorization: &JiraAuthorization,
) -> reqwest::RequestBuilder {
    match authorization {
        JiraAuthorization::Basic { email, api_token } => {
            request.basic_auth(email.as_ref(), Some(api_token.as_ref()))
        }
        JiraAuthorization::Bearer(token) => request.bearer_auth(token),
    }
}

fn oauth_error(error: OAuthError) -> JiraAdapterError {
    match error {
        OAuthError::NotConnected => JiraAdapterError::NotConfigured,
        OAuthError::CredentialsInvalid => JiraAdapterError::CredentialsInvalid,
        OAuthError::PermissionDenied => JiraAdapterError::PermissionDenied,
        OAuthError::NetworkUnavailable => JiraAdapterError::NetworkUnavailable,
        OAuthError::InvalidResponse | OAuthError::InvalidState | OAuthError::Storage => {
            JiraAdapterError::InvalidResponse
        }
    }
}

fn endpoint(base_url: &Url, path: &str) -> Result<Url, JiraAdapterError> {
    Url::parse(&format!(
        "{}{}",
        base_url.as_str().trim_end_matches('/'),
        path
    ))
    .map_err(|_| JiraAdapterError::InvalidResponse)
}

fn ensure_success(status: StatusCode) -> Result<(), JiraAdapterError> {
    match status {
        value if value.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED => Err(JiraAdapterError::CredentialsInvalid),
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => Err(JiraAdapterError::PermissionDenied),
        _ => Err(JiraAdapterError::NetworkUnavailable),
    }
}

fn valid_project(project: &JiraProject) -> bool {
    !project.id.trim().is_empty()
        && project.id.len() <= 128
        && !project.key.trim().is_empty()
        && project.key.len() <= 64
        && !project.name.trim().is_empty()
        && project.name.len() <= 240
}

fn recommended_state(category_key: &str) -> TaskState {
    match category_key {
        "done" | "completed" => TaskState::Completed,
        "indeterminate" | "in-flight" | "in_progress" => TaskState::Active,
        _ => TaskState::Ready,
    }
}

fn jira_issue(issue: JiraIssueResponse) -> Option<JiraIssue> {
    let summary = issue.fields.summary.trim().to_owned();
    let status_name = issue.fields.status.name.trim().to_owned();
    if issue.id.is_empty()
        || issue.id.len() > 128
        || issue.key.is_empty()
        || issue.key.len() > 128
        || summary.is_empty()
        || summary.len() > 240
        || issue.fields.status.id.is_empty()
        || status_name.is_empty()
        || issue.fields.updated.is_empty()
    {
        return None;
    }
    Some(JiraIssue {
        id: issue.id,
        key: issue.key,
        summary,
        status_id: issue.fields.status.id,
        status_name,
        assignee_account_id: issue
            .fields
            .assignee
            .as_ref()
            .and_then(|assignee| assignee.account_id.clone()),
        assignee_name: issue
            .fields
            .assignee
            .and_then(|assignee| assignee.display_name),
        updated_at: issue.fields.updated,
    })
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

    async fn probe(project_status: AxumStatus, profile_status: AxumStatus) -> JiraReadiness {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/rest/api/3/project/search",
                get(move || async move { (project_status, Json(json!({ "values": [] }))) }),
            )
            .route(
                "/rest/api/3/myself",
                get(move || async move { (profile_status, Json(json!({ "displayName": "Bea" }))) }),
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
            probe(AxumStatus::OK, AxumStatus::OK).await.connection,
            JiraConnectionState::Ready
        );
        assert_eq!(
            probe(AxumStatus::UNAUTHORIZED, AxumStatus::OK)
                .await
                .connection,
            JiraConnectionState::CredentialsInvalid
        );
        assert_eq!(
            probe(AxumStatus::FORBIDDEN, AxumStatus::OK)
                .await
                .connection,
            JiraConnectionState::PermissionDenied
        );
        assert_eq!(
            probe(AxumStatus::OK, AxumStatus::UNAUTHORIZED)
                .await
                .connection,
            JiraConnectionState::Ready
        );
    }

    #[tokio::test]
    async fn rejects_insecure_remote_and_embedded_credentials() {
        assert!(JiraReadinessProbe::configured("http://jira.example.test", "a", "b").is_err());
        assert!(JiraReadinessProbe::configured("https://a:b@jira.example.test", "a", "b").is_err());
    }

    async fn configured_catalog() -> JiraReadinessProbe {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/rest/api/3/project/search",
                get(|| async {
                    Json(json!({
                        "isLast": true,
                        "total": 2,
                        "values": [
                            { "id": "10001", "key": "WEB", "name": "Website Services" },
                            { "id": "10002", "key": "OPS", "name": "Operations" }
                        ]
                    }))
                }),
            )
            .route(
                "/rest/api/3/project/WEB/statuses",
                get(|| async {
                    Json(json!([{
                        "statuses": [
                            { "id": "1", "name": "To Do", "statusCategory": { "key": "new" } },
                            { "id": "3", "name": "In Progress", "statusCategory": { "key": "indeterminate" } },
                            { "id": "5", "name": "Done", "statusCategory": { "key": "done" } }
                        ]
                    }, {
                        "statuses": [
                            { "id": "3", "name": "In Progress", "statusCategory": { "key": "indeterminate" } }
                        ]
                    }]))
                }),
            )
            .route(
                "/rest/api/3/search/jql",
                get(|| async {
                    Json(json!({
                        "isLast": true,
                        "issues": [{
                            "id": "20001",
                            "key": "WEB-42",
                            "fields": {
                                "summary": "Polish the launch page",
                                "status": { "id": "3", "name": "In Progress" },
                                "assignee": { "accountId": "account-1", "displayName": "Bea" },
                                "updated": "2026-08-13T13:00:00.000+0000"
                            }
                        }]
                    }))
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn discovers_visible_projects_and_deduplicated_project_statuses() {
        let adapter = configured_catalog().await;
        let projects = adapter.projects(Some("web")).await.unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].key, "WEB");
        let statuses = adapter.project_statuses("WEB").await.unwrap();
        assert_eq!(statuses.len(), 3);
        assert_eq!(statuses[0].recommended_task_state, TaskState::Ready);
        assert_eq!(statuses[1].recommended_task_state, TaskState::Active);
        assert_eq!(statuses[2].recommended_task_state, TaskState::Completed);
        let issues = adapter.issues("10001").await.unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "WEB-42");
        assert_eq!(issues[0].assignee_name.as_deref(), Some("Bea"));
    }
}
