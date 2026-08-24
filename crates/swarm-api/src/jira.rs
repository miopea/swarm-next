use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use swarm_domain::{JiraConnectionState, TaskState};

use crate::jira_oauth::{JiraOAuthClient, OAuthError};

const PAGE_SIZE: usize = 50;
const MAX_PROJECTS: usize = 500;
const MAX_PROJECT_STATUSES: usize = 128;
const MAX_ISSUES: usize = 200;
const MAX_TRANSITIONS: usize = 128;
const MAX_ISSUE_DESCRIPTION_BYTES: usize = 10_000;
const MAX_COMMENTS: usize = 100;
const MAX_ATTACHMENTS: usize = 40;
const MAX_ATTACHMENT_BYTES: usize = 15 * 1024 * 1024;

#[derive(Clone, Default)]
pub(crate) enum JiraReadinessProbe {
    #[default]
    NotConfigured,
    Configured {
        client: Box<Client>,
        /// Swappable while the process runs.
        ///
        /// These used to be read from the environment at start and fixed for
        /// the life of the process, so a fresh Hive could not connect Jira at
        /// all without editing a systemd unit and restarting — which is not
        /// something an operator can do from the settings page they are
        /// looking at. Reported 2026-08-24 from a first install: "she should
        /// be able to run through the auth flow herself with her creds."
        credentials: Arc<tokio::sync::RwLock<Option<JiraCredentials>>>,
    },
    OAuth(JiraOAuthClient),
}

/// What one Atlassian account needs to reach its site.
#[derive(Clone, Debug)]
pub(crate) struct JiraCredentials {
    pub(crate) base_url: Url,
    pub(crate) email: Arc<str>,
    pub(crate) api_token: Arc<str>,
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
    /// Whether this host will take an Atlassian API token typed into Settings.
    /// False when it is wired to an OAuth app at start, where the consent flow
    /// is the way in.
    pub accepts_api_token: bool,
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
    pub description: String,
    pub status_id: String,
    pub status_name: String,
    pub assignee_account_id: Option<String>,
    pub assignee_name: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct JiraComment {
    pub id: String,
    pub author_name: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct JiraIssueAttachment {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub byte_size: usize,
    pub is_image: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct JiraIssueDetail {
    pub summary: String,
    pub description: String,
    pub attachments: Vec<JiraIssueAttachment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JiraAttachmentContent {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JiraTransitionResult {
    pub status_id: String,
    pub status_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JiraAdapterError {
    NotConfigured,
    CredentialsInvalid,
    PermissionDenied,
    NetworkUnavailable,
    InvalidResponse,
    ResponseLimitExceeded,
    TransitionUnavailable,
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
            Self::TransitionUnavailable => {
                "Jira does not offer a mapped transition for this task state"
            }
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct JiraAccount {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
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
    description: Option<serde_json::Value>,
    status: JiraIssueStatus,
    assignee: Option<JiraIssueAssignee>,
    updated: String,
    #[serde(default)]
    attachment: Vec<JiraIssueAttachmentResponse>,
}

#[derive(Deserialize)]
struct JiraIssueAttachmentResponse {
    id: String,
    filename: String,
    #[serde(rename = "mimeType")]
    media_type: String,
    size: usize,
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

#[derive(Deserialize)]
struct JiraTransitionsResponse {
    #[serde(default)]
    transitions: Vec<JiraTransitionResponse>,
}

#[derive(Deserialize)]
struct JiraCommentPage {
    #[serde(default)]
    comments: Vec<JiraCommentResponse>,
}

#[derive(Deserialize)]
struct JiraCommentResponse {
    id: String,
    author: JiraIssueAssignee,
    body: serde_json::Value,
    created: String,
    updated: String,
}

#[derive(Deserialize)]
struct JiraTransitionResponse {
    id: String,
    to: JiraTransitionStatus,
}

#[derive(Deserialize)]
struct JiraTransitionStatus {
    id: String,
    name: String,
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

    pub(crate) async fn browser_base_url(&self) -> Option<Url> {
        match self {
            Self::Configured { credentials, .. } => credentials
                .read()
                .await
                .as_ref()
                .map(|held| held.base_url.clone()),
            Self::OAuth(client) => client.site_url().await,
            Self::NotConfigured => None,
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
        Self::runtime(Some(JiraCredentials {
            base_url,
            email: email.into(),
            api_token: api_token.into(),
        }))
    }

    /// A probe whose credentials can be set later, from the settings page.
    ///
    /// Installed even with nothing configured, so a fresh Hive has somewhere to
    /// put an Atlassian API token without editing a unit file and restarting.
    pub(crate) fn runtime(credentials: Option<JiraCredentials>) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| format!("Jira HTTP client could not start: {error}"))?;
        Ok(Self::Configured {
            client: Box::new(client),
            credentials: Arc::new(tokio::sync::RwLock::new(credentials)),
        })
    }

    /// Replaces the stored account, or clears it. Takes effect on the next
    /// request; nothing restarts.
    pub(crate) async fn set_credentials(&self, next: Option<JiraCredentials>) -> bool {
        match self {
            Self::Configured { credentials, .. } => {
                *credentials.write().await = next;
                true
            }
            Self::NotConfigured | Self::OAuth(_) => false,
        }
    }

    /// Whether this host takes an API token from the settings page, as opposed
    /// to being wired to an Atlassian OAuth app at start.
    pub(crate) const fn accepts_api_token(&self) -> bool {
        matches!(self, Self::Configured { .. })
    }

    pub(crate) async fn readiness(&self) -> JiraReadiness {
        if matches!(self, Self::NotConfigured) {
            return JiraReadiness {
                configured: false,
                accepts_api_token: self.accepts_api_token(),
                connection: JiraConnectionState::NotConnected,
                account_name: None,
            };
        }
        let access = match self.access().await {
            Ok(access) => access,
            Err(JiraAdapterError::NotConfigured) => {
                return JiraReadiness {
                    configured: true,
                    accepts_api_token: self.accepts_api_token(),
                    connection: JiraConnectionState::NotConnected,
                    account_name: None,
                };
            }
            Err(JiraAdapterError::CredentialsInvalid) => {
                return JiraReadiness {
                    configured: true,
                    accepts_api_token: self.accepts_api_token(),
                    connection: JiraConnectionState::CredentialsInvalid,
                    account_name: None,
                };
            }
            Err(JiraAdapterError::PermissionDenied) => {
                return JiraReadiness {
                    configured: true,
                    accepts_api_token: self.accepts_api_token(),
                    connection: JiraConnectionState::PermissionDenied,
                    account_name: None,
                };
            }
            Err(_) => return unavailable(self.accepts_api_token()),
        };
        let Ok(mut project_probe_url) = endpoint(&access.base_url, "/rest/api/3/project/search")
        else {
            return unavailable(self.accepts_api_token());
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
            return unavailable(self.accepts_api_token());
        };
        match project_response.status() {
            StatusCode::UNAUTHORIZED => {
                return JiraReadiness {
                    configured: true,
                    accepts_api_token: self.accepts_api_token(),
                    connection: JiraConnectionState::CredentialsInvalid,
                    account_name: None,
                };
            }
            StatusCode::FORBIDDEN => {
                return JiraReadiness {
                    configured: true,
                    accepts_api_token: self.accepts_api_token(),
                    connection: JiraConnectionState::PermissionDenied,
                    account_name: None,
                };
            }
            status if status.is_success() => {}
            _ => return unavailable(self.accepts_api_token()),
        }

        // Project discovery is the capability Swarm requires. Profile access is
        // cosmetic and older otherwise-valid grants may not include read:jira-user.
        let account_name = account(&access)
            .await
            .ok()
            .and_then(|account| account.display_name)
            .filter(|name| !name.trim().is_empty());
        JiraReadiness {
            configured: true,
            accepts_api_token: self.accepts_api_token(),
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
                    recommended_task_state: recommended_state(&category_key, &status.name),
                    id: status.id,
                    name: status.name,
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
        self.issues_with_scope(project_id, JiraIssueScope::All)
            .await
    }

    pub(crate) async fn hive_intake_issues(
        &self,
        project_id: &str,
    ) -> Result<Vec<JiraIssue>, JiraAdapterError> {
        self.issues_with_scope(project_id, JiraIssueScope::UnassignedOpen)
            .await
    }

    pub(crate) async fn assigned_open_issues(
        &self,
        project_id: &str,
    ) -> Result<Vec<JiraIssue>, JiraAdapterError> {
        self.issues_with_scope(project_id, JiraIssueScope::AssignedToCurrentUserOpen)
            .await
    }

    /// Fetches the exact set of already-linked issues, including terminal Jira states.
    /// This avoids losing an older or closed linked issue behind a project's bounded
    /// newest-first catalog window.
    pub(crate) async fn linked_issues(
        &self,
        issue_ids: &[String],
    ) -> Result<Vec<JiraIssue>, JiraAdapterError> {
        if issue_ids.is_empty() {
            return Ok(Vec::new());
        }
        if issue_ids.len() > MAX_ISSUES
            || issue_ids.iter().any(|id| {
                id.trim().is_empty() || id.len() > 128 || id.chars().any(char::is_control)
            })
        {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let quoted_ids = issue_ids
            .iter()
            .map(|id| {
                format!(
                    "\"{}\"",
                    id.trim().replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.search_issues(&format!("id in ({quoted_ids}) ORDER BY updated DESC"))
            .await
    }

    async fn issues_with_scope(
        &self,
        project_id: &str,
        scope: JiraIssueScope,
    ) -> Result<Vec<JiraIssue>, JiraAdapterError> {
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
        let jql = match scope {
            JiraIssueScope::All => format!("project = {jql_project} ORDER BY updated DESC"),
            JiraIssueScope::UnassignedOpen => format!(
                "project = {jql_project} AND assignee IS EMPTY AND statusCategory != Done ORDER BY updated DESC"
            ),
            JiraIssueScope::AssignedToCurrentUserOpen => format!(
                "project = {jql_project} AND assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC"
            ),
        };
        self.search_issues(&jql).await
    }

    async fn search_issues(&self, jql: &str) -> Result<Vec<JiraIssue>, JiraAdapterError> {
        let access = self.access().await?;
        let mut issues = Vec::new();
        let mut next_page_token: Option<String> = None;
        for _ in 0..(MAX_ISSUES / PAGE_SIZE) {
            let mut url = endpoint(&access.base_url, "/rest/api/3/search/jql")?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("jql", jql);
                pairs.append_pair("maxResults", &PAGE_SIZE.to_string());
                pairs.append_pair("fields", "summary,description,status,assignee,updated");
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
        Ok(issues)
    }

    pub(crate) async fn current_account(&self) -> Result<JiraAccount, JiraAdapterError> {
        let access = self.access().await?;
        account(&access).await
    }

    pub(crate) async fn assign_issue(
        &self,
        issue_id_or_key: &str,
        account_id: &str,
    ) -> Result<(), JiraAdapterError> {
        let issue = issue_id_or_key.trim();
        let account_id = account_id.trim();
        if issue.is_empty()
            || issue.len() > 128
            || issue.chars().any(char::is_control)
            || account_id.is_empty()
            || account_id.len() > 128
            || account_id.chars().any(char::is_control)
        {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, "/rest/api/3/issue/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(issue)
            .push("assignee");
        let response = authorize(access.client.put(url), &access.authorization)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({ "accountId": account_id }))
            .send()
            .await
            .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
        ensure_success(response.status())
    }

    pub(crate) async fn claim_unassigned_issues(
        &self,
        issues: &mut [JiraIssue],
    ) -> Result<(), JiraAdapterError> {
        if !issues
            .iter()
            .any(|issue| issue.assignee_account_id.is_none())
        {
            return Ok(());
        }
        let account = self.current_account().await?;
        for issue in issues
            .iter_mut()
            .filter(|issue| issue.assignee_account_id.is_none())
        {
            self.assign_issue(&issue.id, &account.account_id).await?;
            issue.assignee_account_id = Some(account.account_id.clone());
            issue.assignee_name.clone_from(&account.display_name);
        }
        Ok(())
    }

    pub(crate) async fn comments(
        &self,
        issue_id_or_key: &str,
    ) -> Result<Vec<JiraComment>, JiraAdapterError> {
        let issue = issue_id_or_key.trim();
        if issue.is_empty() || issue.len() > 128 || issue.chars().any(char::is_control) {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, "/rest/api/3/issue/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(issue)
            .push("comment");
        url.query_pairs_mut()
            .append_pair("startAt", "0")
            .append_pair("maxResults", &MAX_COMMENTS.to_string())
            .append_pair("orderBy", "created");
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
        let page = serde_json::from_slice::<JiraCommentPage>(&bytes)
            .map_err(|_| JiraAdapterError::InvalidResponse)?;
        if page.comments.len() > MAX_COMMENTS {
            return Err(JiraAdapterError::ResponseLimitExceeded);
        }
        Ok(page
            .comments
            .into_iter()
            .filter_map(|comment| {
                let body = jira_document_text(&comment.body);
                let author_name = comment
                    .author
                    .display_name
                    .unwrap_or_else(|| "Jira user".into());
                if comment.id.trim().is_empty()
                    || body.is_empty()
                    || comment.created.trim().is_empty()
                    || comment.updated.trim().is_empty()
                {
                    return None;
                }
                Some(JiraComment {
                    id: comment.id,
                    author_name,
                    body,
                    created_at: comment.created,
                    updated_at: comment.updated,
                })
            })
            .collect())
    }

    pub(crate) async fn issue_detail(
        &self,
        issue_id_or_key: &str,
    ) -> Result<JiraIssueDetail, JiraAdapterError> {
        let issue = valid_issue_identifier(issue_id_or_key)?;
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, "/rest/api/3/issue/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(issue);
        url.query_pairs_mut().append_pair(
            "fields",
            "summary,description,attachment,status,assignee,updated",
        );
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
        let response = serde_json::from_slice::<JiraIssueResponse>(&bytes)
            .map_err(|_| JiraAdapterError::InvalidResponse)?;
        let summary = response.fields.summary.trim().to_owned();
        if summary.is_empty() || summary.len() > 240 {
            return Err(JiraAdapterError::InvalidResponse);
        }
        if response.fields.attachment.len() > MAX_ATTACHMENTS {
            return Err(JiraAdapterError::ResponseLimitExceeded);
        }
        let attachments = response
            .fields
            .attachment
            .into_iter()
            .filter_map(valid_attachment)
            .collect();
        Ok(JiraIssueDetail {
            summary,
            description: response
                .fields
                .description
                .as_ref()
                .map(jira_document_text)
                .unwrap_or_default(),
            attachments,
        })
    }

    pub(crate) async fn attachment(
        &self,
        issue_id_or_key: &str,
        attachment_id: &str,
    ) -> Result<JiraAttachmentContent, JiraAdapterError> {
        let attachment_id = valid_attachment_identifier(attachment_id)?;
        let detail = self.issue_detail(issue_id_or_key).await?;
        let attachment = detail
            .attachments
            .into_iter()
            .find(|candidate| candidate.id == attachment_id)
            .filter(|candidate| candidate.is_image)
            .ok_or(JiraAdapterError::PermissionDenied)?;
        if attachment.byte_size > MAX_ATTACHMENT_BYTES {
            return Err(JiraAdapterError::ResponseLimitExceeded);
        }
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, "/rest/api/3/attachment/content/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(attachment_id);
        // Atlassian normally redirects this endpoint to its media host. Asking
        // Jira to proxy the bytes keeps the bounded download on the already
        // authenticated API connection and avoids cross-host redirect/TLS
        // failures in long-running installations.
        url.query_pairs_mut().append_pair("redirect", "false");
        // Atlassian returns HTTP 406 when this non-redirect endpoint receives
        // a media-specific Accept header. Let Jira select the representation,
        // then enforce the declared type and file signature below.
        let response = authorize(access.client.get(url), &access.authorization)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(
                    timeout = error.is_timeout(),
                    connect = error.is_connect(),
                    request = error.is_request(),
                    status = ?error.status(),
                    "Jira attachment content request failed"
                );
                JiraAdapterError::NetworkUnavailable
            })?;
        ensure_success(response.status())?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ATTACHMENT_BYTES as u64)
        {
            return Err(JiraAdapterError::ResponseLimitExceeded);
        }
        let mut bytes = Vec::with_capacity(attachment.byte_size.min(MAX_ATTACHMENT_BYTES));
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| JiraAdapterError::InvalidResponse)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_ATTACHMENT_BYTES {
                return Err(JiraAdapterError::ResponseLimitExceeded);
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() || !matches_image_signature(&attachment.media_type, &bytes) {
            return Err(JiraAdapterError::InvalidResponse);
        }
        Ok(JiraAttachmentContent {
            media_type: attachment.media_type,
            bytes,
        })
    }

    pub(crate) async fn add_comment(
        &self,
        issue_id_or_key: &str,
        body: &str,
    ) -> Result<(), JiraAdapterError> {
        let issue = issue_id_or_key.trim();
        let body = body.trim();
        if issue.is_empty()
            || issue.len() > 128
            || issue.chars().any(char::is_control)
            || body.is_empty()
            || body.len() > 4_000
            || body.chars().any(|character| character == '\0')
        {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, "/rest/api/3/issue/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(issue)
            .push("comment");
        let response = authorize(access.client.post(url), &access.authorization)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({
                "body": {
                    "type": "doc",
                    "version": 1,
                    "content": [{
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": body }]
                    }]
                }
            }))
            .send()
            .await
            .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
        ensure_success(response.status())
    }

    pub(crate) async fn transition_issue(
        &self,
        issue_id_or_key: &str,
        target_status_ids: &[String],
    ) -> Result<JiraTransitionResult, JiraAdapterError> {
        let issue = issue_id_or_key.trim();
        let targets = target_status_ids
            .iter()
            .map(|status| status.trim())
            .filter(|status| !status.is_empty() && status.len() <= 128)
            .collect::<HashSet<_>>();
        if issue.is_empty()
            || issue.len() > 128
            || issue.chars().any(char::is_control)
            || targets.is_empty()
        {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, "/rest/api/3/issue/")?;
        url.path_segments_mut()
            .map_err(|()| JiraAdapterError::InvalidResponse)?
            .pop_if_empty()
            .push(issue)
            .push("transitions");
        let response = authorize(access.client.get(url.clone()), &access.authorization)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
        ensure_success(response.status())?;
        let bytes = response
            .bytes()
            .await
            .map_err(|_| JiraAdapterError::InvalidResponse)?;
        let transitions = serde_json::from_slice::<JiraTransitionsResponse>(&bytes)
            .map_err(|_| JiraAdapterError::InvalidResponse)?
            .transitions;
        if transitions.len() > MAX_TRANSITIONS {
            return Err(JiraAdapterError::ResponseLimitExceeded);
        }
        let transition = transitions
            .into_iter()
            .find(|transition| targets.contains(transition.to.id.trim()))
            .ok_or(JiraAdapterError::TransitionUnavailable)?;
        if transition.id.trim().is_empty()
            || transition.id.len() > 128
            || transition.to.name.trim().is_empty()
            || transition.to.name.len() > 240
        {
            return Err(JiraAdapterError::InvalidResponse);
        }
        let response = authorize(access.client.post(url), &access.authorization)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({ "transition": { "id": transition.id } }))
            .send()
            .await
            .map_err(|_| JiraAdapterError::NetworkUnavailable)?;
        ensure_success(response.status())?;
        Ok(JiraTransitionResult {
            status_id: transition.to.id,
            status_name: transition.to.name,
        })
    }

    async fn access(&self) -> Result<JiraAccess, JiraAdapterError> {
        match self {
            Self::NotConfigured => Err(JiraAdapterError::NotConfigured),
            Self::Configured {
                client,
                credentials,
            } => {
                let held = credentials.read().await;
                let held = held.as_ref().ok_or(JiraAdapterError::NotConfigured)?;
                Ok(JiraAccess {
                    client: client.as_ref().clone(),
                    base_url: held.base_url.clone(),
                    authorization: JiraAuthorization::Basic {
                        email: held.email.clone(),
                        api_token: held.api_token.clone(),
                    },
                })
            }
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

#[derive(Clone, Copy)]
enum JiraIssueScope {
    All,
    UnassignedOpen,
    AssignedToCurrentUserOpen,
}

pub(crate) fn issue_url(base_url: &Url, issue_key: &str) -> Option<String> {
    let issue_key = issue_key.trim();
    if issue_key.is_empty()
        || issue_key.len() > 128
        || issue_key.chars().any(char::is_control)
        || !matches!(base_url.scheme(), "https" | "http")
    {
        return None;
    }
    let mut url = base_url.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .ok()?
        .clear()
        .push("browse")
        .push(issue_key);
    Some(url.into())
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

async fn account(access: &JiraAccess) -> Result<JiraAccount, JiraAdapterError> {
    let url = endpoint(&access.base_url, "/rest/api/3/myself")?;
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
    let account = serde_json::from_slice::<JiraAccount>(&bytes)
        .map_err(|_| JiraAdapterError::InvalidResponse)?;
    if account.account_id.trim().is_empty()
        || account.account_id.len() > 128
        || account.account_id.chars().any(char::is_control)
    {
        return Err(JiraAdapterError::InvalidResponse);
    }
    Ok(account)
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

fn valid_issue_identifier(value: &str) -> Result<&str, JiraAdapterError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(JiraAdapterError::InvalidResponse);
    }
    Ok(value)
}

fn valid_attachment_identifier(value: &str) -> Result<&str, JiraAdapterError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(JiraAdapterError::InvalidResponse);
    }
    Ok(value)
}

fn valid_attachment(value: JiraIssueAttachmentResponse) -> Option<JiraIssueAttachment> {
    let filename = value.filename.trim().to_owned();
    let media_type = value.media_type.trim().to_ascii_lowercase();
    if valid_attachment_identifier(&value.id).is_err()
        || filename.is_empty()
        || filename.len() > 240
        || filename.chars().any(char::is_control)
        || media_type.len() > 128
        || media_type.chars().any(char::is_control)
    {
        return None;
    }
    let is_image = matches!(
        media_type.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    );
    Some(JiraIssueAttachment {
        id: value.id,
        filename,
        media_type,
        byte_size: value.size,
        is_image,
    })
}

fn matches_image_signature(media_type: &str, bytes: &[u8]) -> bool {
    match media_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

/// The lifecycle state a Jira status is offered as, before the operator edits it.
///
/// Jira exposes three status categories, so a recommendation drawn from the
/// category alone can only ever produce three of the six lifecycle states, and
/// blocked, review, and draft become unreachable from the defaults the settings
/// page fills in. A binding built by accepting those defaults cannot express
/// half its own lifecycle, and the failure only surfaces later, when someone
/// tries to move a task to a state no status was mapped to.
///
/// The status name carries what the category cannot. It is only a default, and
/// the operator overrides it in the same dropdown, so a wrong guess costs a
/// correction rather than a broken workflow — which is what the alternative
/// costs today.
fn recommended_state(category_key: &str, status_name: &str) -> TaskState {
    // A finished status is finished, whatever it is called. Jira's done
    // category is unambiguous, and a name like "Approved" or "Reviewed"
    // describes how the work ended rather than something still awaited.
    if matches!(category_key, "done" | "completed") {
        return TaskState::Completed;
    }
    // Below that, the name carries what the category cannot: two categories
    // have to cover four remaining states.
    let name = status_name.trim().to_ascii_lowercase();
    if name.contains("review") || name.contains("proofing") || name.contains("approval") {
        return TaskState::Review;
    }
    if name.contains("block")
        || name.contains("waiting")
        || name.contains("on hold")
        || name.contains("impediment")
    {
        return TaskState::Blocked;
    }
    match category_key {
        "indeterminate" | "in-flight" | "in_progress" => TaskState::Active,
        _ => TaskState::Ready,
    }
}

fn jira_issue(issue: JiraIssueResponse) -> Option<JiraIssue> {
    let summary = issue.fields.summary.trim().to_owned();
    let description = issue
        .fields
        .description
        .as_ref()
        .map(jira_document_text)
        .unwrap_or_default();
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
        description,
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

fn jira_document_text(value: &serde_json::Value) -> String {
    fn visit(value: &serde_json::Value, output: &mut String) {
        match value {
            serde_json::Value::String(text) => output.push_str(text),
            serde_json::Value::Array(items) => {
                for item in items {
                    visit(item, output);
                }
            }
            serde_json::Value::Object(object) => {
                let node_type = object.get("type").and_then(serde_json::Value::as_str);
                if node_type == Some("text") {
                    if let Some(text) = object.get("text").and_then(serde_json::Value::as_str) {
                        output.push_str(text);
                    }
                    return;
                }
                if node_type == Some("hardBreak") {
                    output.push('\n');
                    return;
                }
                if let Some(content) = object.get("content") {
                    visit(content, output);
                }
                if matches!(
                    node_type,
                    Some("paragraph" | "heading" | "listItem" | "blockquote" | "codeBlock")
                ) && !output.ends_with('\n')
                {
                    output.push('\n');
                }
            }
            _ => {}
        }
    }

    let mut output = String::new();
    visit(value, &mut output);
    let normalized = output.trim().replace("\n\n\n", "\n\n");
    if normalized.len() <= MAX_ISSUE_DESCRIPTION_BYTES {
        return normalized;
    }
    let mut boundary = MAX_ISSUE_DESCRIPTION_BYTES;
    while !normalized.is_char_boundary(boundary) {
        boundary -= 1;
    }
    normalized[..boundary].trim_end().to_owned()
}

/// Validates what an operator typed, without contacting anything.
///
/// The same transport rule the environment path enforces: HTTPS, or a loopback
/// host for a local test, and never credentials smuggled into the URL.
pub(crate) fn parse_credentials(
    base_url: &str,
    email: &str,
    api_token: &str,
) -> Result<JiraCredentials, String> {
    let base_url = Url::parse(base_url)
        .map_err(|_| "That Jira site is not a valid URL. It looks like https://yourcompany.atlassian.net".to_owned())?;
    let permitted_transport = base_url.scheme() == "https"
        || (base_url.scheme() == "http"
            && base_url
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost")));
    if !permitted_transport {
        return Err("The Jira site must use https.".to_owned());
    }
    if base_url.username() != "" || base_url.password().is_some() {
        return Err("The Jira site must not contain a username or password.".to_owned());
    }
    Ok(JiraCredentials {
        base_url,
        email: email.into(),
        api_token: api_token.into(),
    })
}

/// The Atlassian account this host uses, kept on the host and nowhere else.
///
/// Stored beside the OAuth token, with the same private permissions, so an
/// operator who typed a token into Settings still has it after a restart.
#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct StoredJiraCredentials {
    pub(crate) base_url: String,
    pub(crate) email: String,
    pub(crate) api_token: String,
}

/// Reads the account a previous session stored, if any.
pub(crate) fn read_stored_credentials(path: &std::path::Path) -> Option<JiraCredentials> {
    let bytes = std::fs::read(path).ok()?;
    let stored: StoredJiraCredentials = serde_json::from_slice(&bytes).ok()?;
    Some(JiraCredentials {
        base_url: Url::parse(&stored.base_url).ok()?,
        email: stored.email.into(),
        api_token: stored.api_token.into(),
    })
}

/// Writes the account privately, or removes it. The token never goes anywhere
/// else — not into the database, not into a log, not back out of the API.
pub(crate) fn write_stored_credentials(
    path: &std::path::Path,
    credentials: Option<&JiraCredentials>,
) -> Result<(), String> {
    let Some(credentials) = credentials else {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("the Jira account could not be cleared: {error}")),
        }
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("the Jira account could not be saved: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let stored = StoredJiraCredentials {
        base_url: credentials.base_url.to_string(),
        email: credentials.email.to_string(),
        api_token: credentials.api_token.to_string(),
    };
    let bytes = serde_json::to_vec(&stored)
        .map_err(|error| format!("the Jira account could not be prepared: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    write_private_bytes(&temporary, &bytes)?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("the Jira account could not be saved: {error}"))
}

#[cfg(unix)]
fn write_private_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("the Jira account could not be saved: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("the Jira account could not be saved: {error}"))
}

#[cfg(not(unix))]
fn write_private_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes)
        .map_err(|error| format!("the Jira account could not be saved: {error}"))
}

fn unavailable(accepts_api_token: bool) -> JiraReadiness {
    JiraReadiness {
        configured: true,
        accepts_api_token,
        connection: JiraConnectionState::NetworkUnavailable,
        account_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::Query, http::StatusCode as AxumStatus, routing::get};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
                get(move || async move {
                    (
                        profile_status,
                        Json(json!({ "accountId": "account-1", "displayName": "Bea" })),
                    )
                }),
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

    #[tokio::test]
    async fn applies_only_an_available_mapped_issue_transition() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let written = writes.clone();
        let app = Router::new().route(
            "/rest/api/3/issue/WEB-42/transitions",
            get(|| async {
                Json(json!({ "transitions": [
                    { "id": "21", "to": { "id": "2", "name": "In Progress" } },
                    { "id": "31", "to": { "id": "3", "name": "Done" } }
                ] }))
            })
            .post(move |Json(body): Json<serde_json::Value>| {
                let written = written.clone();
                async move {
                    assert_eq!(body["transition"]["id"], "21");
                    written.fetch_add(1, Ordering::SeqCst);
                    AxumStatus::NO_CONTENT
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let probe = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        let result = probe
            .transition_issue("WEB-42", &["2".to_owned()])
            .await
            .unwrap();
        assert_eq!(result.status_id, "2");
        assert_eq!(result.status_name, "In Progress");
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            probe.transition_issue("WEB-42", &["99".to_owned()]).await,
            Err(JiraAdapterError::TransitionUnavailable)
        );
        assert_eq!(writes.load(Ordering::SeqCst), 1);
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
                                "description": {
                                    "type": "doc",
                                    "version": 1,
                                    "content": [{
                                        "type": "paragraph",
                                        "content": [
                                            { "type": "text", "text": "Verify desktop" },
                                            { "type": "hardBreak" },
                                            { "type": "text", "text": "and mobile." }
                                        ]
                                    }]
                                },
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
        assert_eq!(issues[0].description, "Verify desktop\nand mobile.");
        assert_eq!(issues[0].assignee_name.as_deref(), Some("Bea"));
    }

    #[test]
    fn every_lifecycle_state_is_reachable_from_the_offered_defaults() {
        // Jira has three status categories, so a recommendation drawn from the
        // category alone can only offer three of the six lifecycle states. A
        // binding built by accepting those defaults cannot express blocked or
        // review, which is how project 10009 ended up untransitionable to
        // either. These are the real status names from that project.
        assert_eq!(
            recommended_state("indeterminate", "In Review"),
            TaskState::Review
        );
        assert_eq!(
            recommended_state("indeterminate", "Proofing"),
            TaskState::Review
        );
        assert_eq!(recommended_state("new", "Waiting On"), TaskState::Blocked);
        assert_eq!(recommended_state("new", "Blocked"), TaskState::Blocked);

        // The category still decides everything the name does not claim.
        assert_eq!(recommended_state("new", "To Do"), TaskState::Ready);
        assert_eq!(recommended_state("new", "Backlog"), TaskState::Ready);
        assert_eq!(
            recommended_state("indeterminate", "In Progress"),
            TaskState::Active
        );
        assert_eq!(recommended_state("done", "Done"), TaskState::Completed);
        // A done status keeps its category even when its name suggests review:
        // finished work is not waiting on anyone. Written the other way round
        // first, which is how the ordering flaw was found.
        assert_eq!(recommended_state("done", "Approved"), TaskState::Completed);
        assert_eq!(recommended_state("done", "Reviewed"), TaskState::Completed);
    }

    #[tokio::test]
    async fn fetches_bounded_issue_detail_and_validated_image_attachment() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/rest/api/3/issue/WEB-42",
                get(|Query(query): Query<HashMap<String, String>>| async move {
                    assert!(query.get("fields").is_some_and(|fields| fields.contains("attachment")));
                    Json(json!({
                        "id": "20001",
                        "key": "WEB-42",
                        "fields": {
                            "summary": "Show the evidence",
                            "description": { "type": "doc", "version": 1, "content": [{
                                "type": "paragraph", "content": [{ "type": "text", "text": "Full Jira detail" }]
                            }] },
                            "status": { "id": "1", "name": "To Do" },
                            "assignee": null,
                            "updated": "2026-08-16T13:00:00.000+0000",
                            "attachment": [{
                                "id": "attachment-1", "filename": "evidence.png", "mimeType": "image/png", "size": 12
                            }]
                        }
                    }))
                }),
            )
            .route(
                "/rest/api/3/attachment/content/attachment-1",
                get(|Query(query): Query<HashMap<String, String>>| async move {
                    assert_eq!(query.get("redirect").map(String::as_str), Some("false"));
                    (
                        [(reqwest::header::CONTENT_TYPE.as_str(), "image/png")],
                        b"\x89PNG\r\n\x1a\nbody".to_vec(),
                    )
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        let detail = adapter.issue_detail("WEB-42").await.unwrap();
        assert_eq!(detail.description, "Full Jira detail");
        assert_eq!(detail.attachments.len(), 1);
        assert!(detail.attachments[0].is_image);
        let image = adapter.attachment("WEB-42", "attachment-1").await.unwrap();
        assert_eq!(image.media_type, "image/png");
        assert!(image.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            adapter.attachment("WEB-42", "not-linked").await,
            Err(JiraAdapterError::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn hive_intake_requests_only_unassigned_open_work() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/rest/api/3/search/jql",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                let jql = query.get("jql").map(String::as_str).unwrap_or_default();
                assert!(jql.contains("project = 10001"));
                assert!(jql.contains("assignee IS EMPTY"));
                assert!(!jql.contains("currentUser()"));
                assert!(jql.contains("statusCategory != Done"));
                Json(json!({ "isLast": true, "issues": [] }))
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        assert!(
            adapter
                .hive_intake_issues("10001")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn assigned_sync_requests_only_current_users_open_work() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/rest/api/3/search/jql",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                let jql = query.get("jql").map(String::as_str).unwrap_or_default();
                assert!(jql.contains("project = 10001"));
                assert!(jql.contains("assignee = currentUser()"));
                assert!(!jql.contains("assignee IS EMPTY"));
                assert!(jql.contains("statusCategory != Done"));
                Json(json!({ "isLast": true, "issues": [] }))
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        assert!(
            adapter
                .assigned_open_issues("10001")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn linked_sync_requests_exact_issues_without_excluding_done() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/rest/api/3/search/jql",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                let jql = query.get("jql").map(String::as_str).unwrap_or_default();
                assert!(jql.contains("id in (\"20001\", \"20002\")"));
                assert!(!jql.contains("statusCategory != Done"));
                Json(json!({ "isLast": true, "issues": [] }))
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        assert!(
            adapter
                .linked_issues(&["20001".to_owned(), "20002".to_owned()])
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn assigns_an_issue_to_the_explicit_connected_account() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let written = writes.clone();
        let app = Router::new()
            .route(
                "/rest/api/3/myself",
                get(|| async { Json(json!({ "accountId": "account-1", "displayName": "Bea" })) }),
            )
            .route(
                "/rest/api/3/issue/WEB-42/assignee",
                axum::routing::put(move |Json(body): Json<serde_json::Value>| {
                    let written = written.clone();
                    async move {
                        assert_eq!(body["accountId"], "account-1");
                        written.fetch_add(1, Ordering::SeqCst);
                        AxumStatus::NO_CONTENT
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        let account = adapter.current_account().await.unwrap();
        assert_eq!(account.account_id, "account-1");
        adapter
            .assign_issue("WEB-42", &account.account_id)
            .await
            .unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn reads_and_writes_bounded_rich_text_comments() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let written = writes.clone();
        let app = Router::new().route(
            "/rest/api/3/issue/WEB-42/comment",
            get(|| async {
                Json(json!({
                    "comments": [{
                        "id": "comment-1",
                        "author": { "accountId": "account-1", "displayName": "Bea" },
                        "body": { "type": "doc", "version": 1, "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Ready for review" }]
                        }]},
                        "created": "2026-08-13T12:00:00.000+0000",
                        "updated": "2026-08-13T12:00:00.000+0000"
                    }]
                }))
            })
            .post(move |Json(body): Json<serde_json::Value>| {
                let written = written.clone();
                async move {
                    assert_eq!(
                        body["body"]["content"][0]["content"][0]["text"],
                        "Shipped cleanly"
                    );
                    written.fetch_add(1, Ordering::SeqCst);
                    AxumStatus::CREATED
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = JiraReadinessProbe::configured(
            &format!("http://{address}"),
            "operator@example.test",
            "token",
        )
        .unwrap();

        let comments = adapter.comments("WEB-42").await.unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].author_name, "Bea");
        assert_eq!(comments[0].body, "Ready for review");
        adapter
            .add_comment("WEB-42", "Shipped cleanly")
            .await
            .unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
    }
}
