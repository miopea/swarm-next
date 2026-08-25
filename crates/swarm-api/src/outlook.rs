use base64ct::{Base64, Encoding};
use chrono::DateTime;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};

use crate::microsoft_oauth::{MicrosoftAccess, MicrosoftOAuthClient, OAuthError};

const MAX_INBOX_MESSAGES: usize = 50;
const MAX_ATTACHMENTS: usize = 16;
const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_BODY_BYTES: usize = 100_000;
const MAX_QUERY_BYTES: usize = 256;

#[derive(Clone, Default)]
pub(crate) enum OutlookProbe {
    #[default]
    NotConfigured,
    OAuth(MicrosoftOAuthClient),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OutlookReadiness {
    pub configured: bool,
    pub connection: OutlookConnectionState,
    pub account_name: Option<String>,
    pub account_address: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutlookConnectionState {
    NotConnected,
    Ready,
    CredentialsInvalid,
    PermissionDenied,
    NetworkUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct OutlookMessageSummary {
    pub id: String,
    pub conversation_id: String,
    pub internet_message_id: Option<String>,
    pub subject: String,
    pub sender_name: String,
    pub sender_address: String,
    pub received_at: i64,
    pub web_url: String,
    pub has_attachments: bool,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OutlookMessage {
    #[serde(skip)]
    pub integration_id: String,
    pub summary: OutlookMessageSummary,
    pub body_text: String,
    pub attachments: Vec<OutlookAttachment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct OutlookAttachment {
    pub id: String,
    pub name: String,
    pub media_type: String,
    pub byte_size: u64,
    pub inline: bool,
    pub content_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutlookAttachmentContent {
    pub metadata: OutlookAttachment,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutlookError {
    NotConfigured,
    CredentialsInvalid,
    PermissionDenied,
    NetworkUnavailable,
    NotFound,
    InvalidRequest,
    InvalidResponse,
    ResponseLimitExceeded,
    UnsupportedAttachment,
    AmbiguousDelivery,
}

impl std::fmt::Display for OutlookError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotConfigured => "Email is not connected",
            Self::CredentialsInvalid => "Microsoft credentials are invalid",
            Self::PermissionDenied => "Microsoft denied mailbox access",
            Self::NetworkUnavailable => "Microsoft Outlook is temporarily unavailable",
            Self::NotFound => "The email message was not found",
            Self::InvalidRequest => "The email request was invalid",
            Self::InvalidResponse => "Microsoft Outlook returned an invalid response",
            Self::ResponseLimitExceeded => "The email response exceeded Swarm's safety limit",
            Self::UnsupportedAttachment => "The email attachment is not supported",
            Self::AmbiguousDelivery => "The reply result could not be confirmed",
        })
    }
}

#[derive(Deserialize)]
struct MessagePage {
    #[serde(default)]
    value: Vec<GraphMessage>,
}

#[derive(Deserialize)]
struct GraphMessage {
    id: String,
    #[serde(rename = "conversationId")]
    conversation_id: String,
    #[serde(rename = "internetMessageId")]
    internet_message_id: Option<String>,
    #[serde(default)]
    subject: String,
    from: Option<GraphRecipient>,
    #[serde(rename = "receivedDateTime")]
    received_at: String,
    #[serde(rename = "webLink")]
    web_url: String,
    #[serde(rename = "hasAttachments", default)]
    has_attachments: bool,
    #[serde(rename = "bodyPreview", default)]
    preview: String,
    body: Option<GraphBody>,
}

#[derive(Deserialize)]
struct GraphRecipient {
    #[serde(rename = "emailAddress")]
    email_address: GraphAddress,
}

#[derive(Deserialize)]
struct GraphAddress {
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: String,
}

#[derive(Deserialize)]
struct GraphBody {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct AttachmentPage {
    #[serde(default)]
    value: Vec<GraphAttachment>,
}

#[derive(Deserialize)]
struct GraphAttachment {
    #[serde(rename = "@odata.type", default)]
    kind: String,
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "contentType", default)]
    media_type: String,
    #[serde(default)]
    size: u64,
    #[serde(rename = "isInline", default)]
    inline: bool,
    #[serde(rename = "contentId")]
    content_id: Option<String>,
    #[serde(rename = "contentBytes")]
    content_bytes: Option<String>,
}

impl OutlookProbe {
    pub(crate) fn oauth(client: MicrosoftOAuthClient) -> Self {
        Self::OAuth(client)
    }

    pub(crate) fn oauth_client(&self) -> Option<&MicrosoftOAuthClient> {
        if let Self::OAuth(client) = self {
            Some(client)
        } else {
            None
        }
    }

    pub(crate) async fn readiness(&self) -> OutlookReadiness {
        if matches!(self, Self::NotConfigured) {
            return readiness(false, OutlookConnectionState::NotConnected, None);
        }
        match self.access().await {
            Ok(access) => readiness(true, OutlookConnectionState::Ready, Some(&access)),
            Err(OutlookError::NotConfigured) => {
                readiness(true, OutlookConnectionState::NotConnected, None)
            }
            Err(OutlookError::CredentialsInvalid) => {
                readiness(true, OutlookConnectionState::CredentialsInvalid, None)
            }
            Err(OutlookError::PermissionDenied) => {
                readiness(true, OutlookConnectionState::PermissionDenied, None)
            }
            Err(_) => readiness(true, OutlookConnectionState::NetworkUnavailable, None),
        }
    }

    pub(crate) async fn inbox(
        &self,
        query: Option<&str>,
    ) -> Result<Vec<OutlookMessageSummary>, OutlookError> {
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        if query.is_some_and(|value| {
            value.len() > MAX_QUERY_BYTES || value.chars().any(char::is_control)
        }) {
            return Err(OutlookError::InvalidRequest);
        }
        let access = self.access().await?;
        let mut url = endpoint(
            &access.base_url,
            &["me", "mailFolders", "inbox", "messages"],
        )?;
        url.query_pairs_mut()
            .append_pair("$top", &MAX_INBOX_MESSAGES.to_string())
            .append_pair("$orderby", "receivedDateTime desc")
            .append_pair(
                "$select",
                "id,conversationId,internetMessageId,subject,from,receivedDateTime,webLink,hasAttachments,bodyPreview",
            );
        let page = graph_json::<MessagePage>(&access, url, false).await?;
        if page.value.len() > MAX_INBOX_MESSAGES {
            return Err(OutlookError::ResponseLimitExceeded);
        }
        let needle = query.map(str::to_lowercase);
        page.value
            .into_iter()
            .map(message_summary)
            .collect::<Result<Vec<_>, _>>()
            .map(|messages| {
                messages
                    .into_iter()
                    .filter(|message| {
                        needle.as_ref().is_none_or(|needle| {
                            message.subject.to_lowercase().contains(needle)
                                || message.sender_name.to_lowercase().contains(needle)
                                || message.sender_address.to_lowercase().contains(needle)
                                || message.preview.to_lowercase().contains(needle)
                        })
                    })
                    .collect()
            })
    }

    pub(crate) async fn message(&self, message_id: &str) -> Result<OutlookMessage, OutlookError> {
        validate_identifier(message_id)?;
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, &["me", "messages", message_id])?;
        url.query_pairs_mut().append_pair(
            "$select",
            "id,conversationId,internetMessageId,subject,from,receivedDateTime,webLink,hasAttachments,bodyPreview,body",
        );
        let graph_message = graph_json::<GraphMessage>(&access, url, true).await?;
        let body_text = graph_message
            .body
            .as_ref()
            .map_or("", |body| body.content.trim())
            .to_owned();
        if body_text.len() > MAX_BODY_BYTES {
            return Err(OutlookError::ResponseLimitExceeded);
        }
        let attachments = if graph_message.has_attachments {
            self.attachments_with_access(&access, message_id).await?
        } else {
            Vec::new()
        };
        Ok(OutlookMessage {
            integration_id: access.integration_id.clone(),
            summary: message_summary(graph_message)?,
            body_text,
            attachments,
        })
    }

    pub(crate) async fn attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<OutlookAttachmentContent, OutlookError> {
        validate_identifier(message_id)?;
        validate_identifier(attachment_id)?;
        let access = self.access().await?;
        let url = endpoint(
            &access.base_url,
            &["me", "messages", message_id, "attachments", attachment_id],
        )?;
        let attachment = graph_json::<GraphAttachment>(&access, url, false).await?;
        let metadata = attachment_metadata(&attachment)?;
        let encoded = attachment
            .content_bytes
            .ok_or(OutlookError::UnsupportedAttachment)?;
        if encoded.len() > MAX_ATTACHMENT_BYTES.saturating_mul(2) {
            return Err(OutlookError::ResponseLimitExceeded);
        }
        let bytes = Base64::decode_vec(&encoded).map_err(|_| OutlookError::InvalidResponse)?;
        if bytes.is_empty()
            || bytes.len() > MAX_ATTACHMENT_BYTES
            || bytes.len() as u64 > metadata.byte_size.saturating_add(3)
        {
            return Err(OutlookError::ResponseLimitExceeded);
        }
        Ok(OutlookAttachmentContent { metadata, bytes })
    }

    /// The message's CURRENT Graph id, found by its stable internet id.
    ///
    /// A Graph `id` is folder-scoped and CHANGES when a message moves — filed,
    /// archived, or swept by a rule. The internet message id is the RFC 5322
    /// Message-ID and does not change. Storing the first and replying to it
    /// months later is why every reply this Hive sent on 2026-08-25 came back
    /// 404 and was cancelled: seventeen targets, none delivered, and the
    /// operator found out by looking in Outlook and seeing nothing.
    ///
    /// A 404 there was never a token or mailbox problem — those are 401 and 403.
    /// It meant authenticated, and that exact id is not in this mailbox any
    /// more.
    ///
    /// Returns `NotFound` only when the message genuinely is not there, which
    /// is then an honest fact about the mailbox rather than something to retry.
    pub(crate) async fn message_id_for_internet_id(
        &self,
        internet_message_id: &str,
    ) -> Result<String, OutlookError> {
        if internet_message_id.is_empty()
            || internet_message_id.len() > 998
            || internet_message_id.contains('\'')
            || internet_message_id.chars().any(char::is_control)
        {
            return Err(OutlookError::InvalidRequest);
        }
        let access = self.access().await?;
        let mut url = endpoint(&access.base_url, &["me", "messages"])?;
        url.query_pairs_mut()
            .append_pair(
                "$filter",
                &format!("internetMessageId eq '{internet_message_id}'"),
            )
            .append_pair("$select", "id")
            .append_pair("$top", "1");
        let response = access
            .client
            .get(url)
            .bearer_auth(&access.access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| OutlookError::NetworkUnavailable)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => return Err(OutlookError::CredentialsInvalid),
            StatusCode::FORBIDDEN => return Err(OutlookError::PermissionDenied),
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                return Err(OutlookError::NetworkUnavailable);
            }
            _ => return Err(OutlookError::InvalidResponse),
        }
        let payload = response
            .json::<serde_json::Value>()
            .await
            .map_err(|_| OutlookError::InvalidResponse)?;
        payload
            .get("value")
            .and_then(|value| value.as_array())
            .and_then(|messages| messages.first())
            .and_then(|message| message.get("id"))
            .and_then(|id| id.as_str())
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .ok_or(OutlookError::NotFound)
    }

    pub(crate) async fn reply(&self, message_id: &str, body: &str) -> Result<String, OutlookError> {
        validate_identifier(message_id)?;
        let body = body.trim();
        if body.is_empty() || body.len() > 10_000 || body.chars().any(|character| character == '\0')
        {
            return Err(OutlookError::InvalidRequest);
        }
        let access = self.access().await?;
        let url = endpoint(&access.base_url, &["me", "messages", message_id, "reply"])?;
        let response = access
            .client
            .post(url)
            .bearer_auth(&access.access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({ "comment": body }))
            .send()
            .await
            .map_err(|_| OutlookError::AmbiguousDelivery)?;
        match response.status() {
            StatusCode::ACCEPTED | StatusCode::NO_CONTENT => Ok(format!("graph:{message_id}")),
            StatusCode::UNAUTHORIZED => Err(OutlookError::CredentialsInvalid),
            StatusCode::FORBIDDEN => Err(OutlookError::PermissionDenied),
            StatusCode::NOT_FOUND => Err(OutlookError::NotFound),
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                Err(OutlookError::AmbiguousDelivery)
            }
            _ => Err(OutlookError::InvalidResponse),
        }
    }

    async fn attachments_with_access(
        &self,
        access: &MicrosoftAccess,
        message_id: &str,
    ) -> Result<Vec<OutlookAttachment>, OutlookError> {
        let mut url = endpoint(
            &access.base_url,
            &["me", "messages", message_id, "attachments"],
        )?;
        url.query_pairs_mut()
            .append_pair("$top", &MAX_ATTACHMENTS.to_string())
            // `contentId` belongs to `fileAttachment`, not Graph's base
            // `attachment` resource. Selecting it (or `@odata.type`) on the
            // collection makes Graph reject every message with attachments.
            // The type discriminator is returned automatically, while the
            // concrete attachment download supplies `contentId` during import.
            .append_pair("$select", "id,name,contentType,size,isInline");
        let page = graph_json::<AttachmentPage>(access, url, false).await?;
        if page.value.len() > MAX_ATTACHMENTS {
            return Err(OutlookError::ResponseLimitExceeded);
        }
        page.value.iter().map(attachment_metadata).collect()
    }

    async fn access(&self) -> Result<MicrosoftAccess, OutlookError> {
        match self {
            Self::NotConfigured => Err(OutlookError::NotConfigured),
            Self::OAuth(client) => client.access().await.map_err(map_oauth_error),
        }
    }
}

fn readiness(
    configured: bool,
    connection: OutlookConnectionState,
    access: Option<&MicrosoftAccess>,
) -> OutlookReadiness {
    OutlookReadiness {
        configured,
        connection,
        account_name: access.map(|access| access.account_name.clone()),
        account_address: access.map(|access| access.account_address.clone()),
    }
}

fn message_summary(message: GraphMessage) -> Result<OutlookMessageSummary, OutlookError> {
    validate_identifier(&message.id)?;
    validate_identifier(&message.conversation_id)?;
    let sender = message
        .from
        .ok_or(OutlookError::InvalidResponse)?
        .email_address;
    let received_at = DateTime::parse_from_rfc3339(&message.received_at)
        .map_err(|_| OutlookError::InvalidResponse)?
        .timestamp();
    if received_at <= 0
        || sender.address.trim().is_empty()
        || sender.address.len() > 320
        || message.subject.len() > 2_000
        || message.preview.len() > 10_000
    {
        return Err(OutlookError::InvalidResponse);
    }
    let web_url = Url::parse(message.web_url.trim()).map_err(|_| OutlookError::InvalidResponse)?;
    if web_url.scheme() != "https" || !web_url.username().is_empty() || web_url.password().is_some()
    {
        return Err(OutlookError::InvalidResponse);
    }
    Ok(OutlookMessageSummary {
        id: message.id,
        conversation_id: message.conversation_id,
        internet_message_id: message.internet_message_id,
        subject: message.subject.trim().to_owned(),
        sender_name: sender.name.trim().to_owned(),
        sender_address: sender.address.trim().to_owned(),
        received_at,
        web_url: web_url.to_string(),
        has_attachments: message.has_attachments,
        preview: message.preview.trim().to_owned(),
    })
}

fn attachment_metadata(value: &GraphAttachment) -> Result<OutlookAttachment, OutlookError> {
    validate_identifier(&value.id)?;
    if value.kind != "#microsoft.graph.fileAttachment" {
        return Err(OutlookError::UnsupportedAttachment);
    }
    if value.name.trim().is_empty()
        || value.name.len() > 255
        || value.name.chars().any(char::is_control)
        || value.media_type.trim().is_empty()
        || value.media_type.len() > 127
        || value.media_type.chars().any(char::is_control)
        || value.size == 0
        || value.size > MAX_ATTACHMENT_BYTES as u64
    {
        return Err(OutlookError::ResponseLimitExceeded);
    }
    Ok(OutlookAttachment {
        id: value.id.clone(),
        name: value.name.trim().to_owned(),
        media_type: value.media_type.trim().to_owned(),
        byte_size: value.size,
        inline: value.inline,
        content_id: value.content_id.clone().filter(|value| !value.is_empty()),
    })
}

async fn graph_json<T: for<'de> Deserialize<'de>>(
    access: &MicrosoftAccess,
    url: Url,
    plain_text: bool,
) -> Result<T, OutlookError> {
    let mut request = access
        .client
        .get(url)
        .bearer_auth(&access.access_token)
        .header(reqwest::header::ACCEPT, "application/json");
    if plain_text {
        request = request.header("Prefer", "outlook.body-content-type=\"text\"");
    }
    let response = request
        .send()
        .await
        .map_err(|_| OutlookError::NetworkUnavailable)?;
    match response.status() {
        StatusCode::UNAUTHORIZED => return Err(OutlookError::CredentialsInvalid),
        StatusCode::FORBIDDEN => return Err(OutlookError::PermissionDenied),
        StatusCode::NOT_FOUND => return Err(OutlookError::NotFound),
        status if status.is_success() => {}
        status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
            return Err(OutlookError::NetworkUnavailable);
        }
        _ => return Err(OutlookError::InvalidResponse),
    }
    response
        .json::<T>()
        .await
        .map_err(|_| OutlookError::InvalidResponse)
}

fn endpoint(base_url: &Url, segments: &[&str]) -> Result<Url, OutlookError> {
    let mut url = base_url.clone();
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|()| OutlookError::InvalidRequest)?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url)
}

fn validate_identifier(value: &str) -> Result<(), OutlookError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        Err(OutlookError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn map_oauth_error(error: OAuthError) -> OutlookError {
    match error {
        OAuthError::NotConnected => OutlookError::NotConfigured,
        OAuthError::CredentialsInvalid => OutlookError::CredentialsInvalid,
        OAuthError::PermissionDenied => OutlookError::PermissionDenied,
        OAuthError::NetworkUnavailable => OutlookError::NetworkUnavailable,
        OAuthError::InvalidResponse | OAuthError::InvalidState | OAuthError::Storage => {
            OutlookError::InvalidResponse
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::{
        Json, Router,
        body::Bytes,
        extract::{Path, Query},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use reqwest::Client;
    use serde_json::json;

    use super::*;

    async fn connected_probe() -> (OutlookProbe, Url, tempfile::TempDir) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/token", post(|| async { Json(json!({"access_token":"access","refresh_token":"refresh","expires_in":3600})) }))
            .route("/me", get(|| async { Json(json!({"id":"account-1","displayName":"Operator","mail":"operator@example.test","userPrincipalName":"operator@example.test"})) }))
            .route("/me/mailFolders/inbox/messages", get(|headers: HeaderMap| async move {
                assert_eq!(headers.get("authorization").unwrap(), "Bearer access");
                Json(json!({"value":[{"id":"message-1","conversationId":"conversation-1","internetMessageId":"<one@example.test>","subject":"Website issue","from":{"emailAddress":{"name":"Reporter","address":"reporter@example.test"}},"receivedDateTime":"2026-08-14T12:30:00Z","webLink":"https://outlook.office.com/mail/message-1","hasAttachments":true,"bodyPreview":"The page is broken"}]}))
            }))
            .route("/me/messages/{id}", get(|Path(id): Path<String>, headers: HeaderMap| async move {
                assert_eq!(id, "message-1");
                assert_eq!(headers.get("prefer").unwrap(), "outlook.body-content-type=\"text\"");
                Json(json!({"id":"message-1","conversationId":"conversation-1","internetMessageId":"<one@example.test>","subject":"Website issue","from":{"emailAddress":{"name":"Reporter","address":"reporter@example.test"}},"receivedDateTime":"2026-08-14T12:30:00Z","webLink":"https://outlook.office.com/mail/message-1","hasAttachments":true,"bodyPreview":"The page is broken","body":{"contentType":"text","content":"The page is broken in production."}}))
            }))
            .route("/me/messages/{id}/attachments", get(|Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(query.get("$top").map(String::as_str), Some("16"));
                assert_eq!(
                    query.get("$select").map(String::as_str),
                    Some("id,name,contentType,size,isInline")
                );
                Json(json!({"value":[{"@odata.type":"#microsoft.graph.fileAttachment","id":"attachment-1","name":"screen.png","contentType":"image/png","size":15,"isInline":false}]}))
            }))
            .route("/me/messages/{id}/attachments/{attachment_id}", get(|| async { Json(json!({"@odata.type":"#microsoft.graph.fileAttachment","id":"attachment-1","name":"screen.png","contentType":"image/png","size":15,"isInline":false,"contentId":null,"contentBytes":"iVBORw0KGgpwcml2YXRl"})) }))
            .route("/me/messages/{id}/reply", post(|Path(id): Path<String>, body: Bytes| async move {
                assert_eq!(id, "message-1");
                assert!(String::from_utf8_lossy(&body).contains("Shipped and verified"));
                StatusCode::ACCEPTED
            }));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let oauth = MicrosoftOAuthClient::new_with_endpoints(
            Client::new(),
            "client-id",
            "client-secret",
            "https://swarm.example.test/",
            directory.path().join("email-oauth.json"),
            "https://login.microsoft.test/authorize",
            &format!("http://{address}/token"),
            &format!("http://{address}/"),
        )
        .unwrap();
        let authorization = oauth.authorization_url().await.unwrap();
        let state = authorization
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        oauth.exchange_code(&state, "code").await.unwrap();
        (
            OutlookProbe::oauth(oauth),
            Url::parse(&format!("http://{address}/")).unwrap(),
            directory,
        )
    }

    #[tokio::test]
    async fn lists_previews_downloads_and_replies_with_bounded_graph_calls() {
        let (probe, _, directory) = connected_probe().await;
        let directory_path = directory.path().to_path_buf();
        let readiness = probe.readiness().await;
        assert_eq!(readiness.connection, OutlookConnectionState::Ready);
        let inbox = probe.inbox(Some("reporter")).await.unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].subject, "Website issue");
        let message = probe.message("message-1").await.unwrap();
        assert_eq!(message.body_text, "The page is broken in production.");
        assert_eq!(message.attachments.len(), 1);
        let attachment = probe.attachment("message-1", "attachment-1").await.unwrap();
        assert!(attachment.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            probe
                .reply("message-1", "Shipped and verified")
                .await
                .unwrap(),
            "graph:message-1"
        );
        drop(probe);
        drop(directory);
        assert!(!directory_path.exists());
    }

    #[tokio::test]
    async fn rejects_unbounded_search_and_identifiers() {
        let (probe, _, _directory) = connected_probe().await;
        assert_eq!(
            probe.inbox(Some(&"x".repeat(MAX_QUERY_BYTES + 1))).await,
            Err(OutlookError::InvalidRequest)
        );
        assert_eq!(
            probe.message("line\nbreak").await,
            Err(OutlookError::InvalidRequest)
        );
    }
}
