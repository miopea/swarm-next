use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Client, Method, StatusCode, Url};
use serde::{Serialize, de::DeserializeOwned};
use swarm_domain::{
    ApiaryJoinLinkId, ApiaryJoinLinkPoll, FederationCatalogSnapshot, FederationClaimId,
    FederationJoinAcceptance, FederationJoinSubmission, FederationSharedClaim,
    FederationTaskCommand, FederationTaskCommandReceipt, FederationTaskPage, HiveConnectionCard,
};
use thiserror::Error;

const FEDERATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FEDERATION_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_KEEPER_ENDPOINT_BYTES: usize = 2_048;

#[derive(Clone)]
pub struct FederationHttpClient {
    client: Client,
    base_url: Url,
}

#[derive(Clone, Copy, Debug, Error)]
pub enum FederationHttpError {
    #[error("Keeper endpoint must be a bounded HTTPS URL without credentials, query, or fragment")]
    InvalidEndpoint,
    #[error("Keeper is temporarily unavailable")]
    TransportUnavailable,
    #[error("Keeper rejected this Hive's federation credential")]
    AuthenticationRejected,
    #[error("Keeper rejected the request because shared state changed")]
    Conflict,
    #[error("Keeper rejected the federation request with HTTP {0}")]
    RemoteRejected(u16),
    #[error("Keeper response exceeded the federation response bound")]
    ResponseTooLarge,
    #[error("Keeper returned an invalid federation response")]
    InvalidResponse,
}

#[derive(Serialize)]
struct ReserveClaimRequest<'a> {
    project_id: &'a str,
    issue_id: &'a str,
    issue_key: &'a str,
}

#[derive(Serialize)]
struct BootstrapRequest<'a> {
    secret: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    connection_card: Option<&'a HiveConnectionCard>,
}

impl FederationHttpClient {
    /// Creates a bounded, redirect-free transport for one signed Keeper base
    /// endpoint. Plain HTTP is accepted only for loopback test/development
    /// peers; remote federation always requires HTTPS.
    ///
    /// # Errors
    /// Rejects malformed, credential-bearing, insecure remote, or oversized
    /// endpoints and failures to construct the bounded HTTP client.
    pub fn new(keeper_endpoint: &str) -> Result<Self, FederationHttpError> {
        Self::with_timeout(keeper_endpoint, FEDERATION_REQUEST_TIMEOUT)
    }

    fn with_timeout(
        keeper_endpoint: &str,
        request_timeout: Duration,
    ) -> Result<Self, FederationHttpError> {
        let base_url = validate_keeper_endpoint(keeper_endpoint)?;
        let client = Client::builder()
            .connect_timeout(request_timeout)
            .timeout(request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("swarm-next-federation/1")
            .build()
            .map_err(|_| FederationHttpError::TransportUnavailable)?;
        Ok(Self { client, base_url })
    }

    /// Delivers one already sealed join submission. The caller remains
    /// responsible for explicit operator authorization before invoking this
    /// method because the request contains the one-time invitation secret.
    ///
    /// # Errors
    /// Returns a typed transport, authentication, conflict, response-bound, or
    /// protocol error. No retry is performed implicitly.
    pub async fn join(
        &self,
        submission: &FederationJoinSubmission,
    ) -> Result<FederationJoinAcceptance, FederationHttpError> {
        self.send_json(
            Method::POST,
            "api/v1/federation/join",
            None,
            Some(submission),
        )
        .await
    }

    /// Presents or polls one Keeper-created bootstrap capability. All traffic
    /// is initiated by the member Hive; the Keeper never connects inward.
    ///
    /// # Errors
    /// Returns a typed transport, authentication, conflict, response-bound, or
    /// protocol error. No retry is performed implicitly.
    pub async fn bootstrap(
        &self,
        link_id: ApiaryJoinLinkId,
        secret: &str,
        connection_card: Option<&HiveConnectionCard>,
    ) -> Result<ApiaryJoinLinkPoll, FederationHttpError> {
        self.send_json(
            Method::POST,
            &format!("api/v1/federation/bootstrap/{link_id}"),
            None,
            Some(&BootstrapRequest {
                secret,
                connection_card,
            }),
        )
        .await
    }

    /// Fetches one signed catalog snapshot for the authenticated member node.
    ///
    /// # Errors
    /// Returns a typed transport, authentication, conflict, response-bound, or
    /// protocol error. No retry is performed implicitly.
    pub async fn catalog(
        &self,
        node_credential: &str,
    ) -> Result<FederationCatalogSnapshot, FederationHttpError> {
        self.send_json::<(), _>(
            Method::GET,
            "api/v1/federation/catalog",
            Some(node_credential),
            None,
        )
        .await
    }

    /// Fetches one ordered page of Keeper-canonical Swarm tasks. Jira issue
    /// content is never returned by this endpoint.
    ///
    /// # Errors
    /// Returns typed transport, authentication, response-bound, or protocol failures.
    pub async fn tasks(
        &self,
        node_credential: &str,
        after: i64,
    ) -> Result<FederationTaskPage, FederationHttpError> {
        self.send_json::<(), _>(
            Method::GET,
            &format!("api/v1/federation/tasks?after={after}"),
            Some(node_credential),
            None,
        )
        .await
    }

    /// Delivers one durable Member command to Keeper without implicit retry.
    ///
    /// # Errors
    /// Returns typed transport, authentication, response-bound, or protocol failures.
    pub async fn submit_task_command(
        &self,
        node_credential: &str,
        command: &FederationTaskCommand,
    ) -> Result<FederationTaskCommandReceipt, FederationHttpError> {
        self.send_json(
            Method::POST,
            "api/v1/federation/tasks/commands",
            Some(node_credential),
            Some(command),
        )
        .await
    }

    /// Requests one bounded shared-issue reservation from the Keeper.
    ///
    /// # Errors
    /// Returns a typed transport, authentication, claim-conflict,
    /// response-bound, or protocol error. No Jira operation is performed.
    pub async fn reserve_claim(
        &self,
        node_credential: &str,
        project_id: &str,
        issue_id: &str,
        issue_key: &str,
    ) -> Result<FederationSharedClaim, FederationHttpError> {
        let request = ReserveClaimRequest {
            project_id,
            issue_id,
            issue_key,
        };
        self.send_json(
            Method::POST,
            "api/v1/federation/claims",
            Some(node_credential),
            Some(&request),
        )
        .await
    }

    /// Confirms one reservation after the caller's Jira adapter succeeds.
    ///
    /// # Errors
    /// Returns a typed transport, authentication, claim-conflict,
    /// response-bound, or protocol error.
    pub async fn confirm_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
    ) -> Result<FederationSharedClaim, FederationHttpError> {
        self.send_json::<(), _>(
            Method::POST,
            &format!("api/v1/federation/claims/{claim_id}/confirmation"),
            Some(node_credential),
            None,
        )
        .await
    }

    /// Releases one unconfirmed reservation after the caller's Jira operation
    /// fails.
    ///
    /// # Errors
    /// Returns a typed transport, authentication, claim-conflict,
    /// response-bound, or protocol error.
    pub async fn release_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
    ) -> Result<FederationSharedClaim, FederationHttpError> {
        self.send_json::<(), _>(
            Method::DELETE,
            &format!("api/v1/federation/claims/{claim_id}"),
            Some(node_credential),
            None,
        )
        .await
    }

    async fn send_json<B, R>(
        &self,
        method: Method,
        path: &str,
        node_credential: Option<&str>,
        body: Option<&B>,
    ) -> Result<R, FederationHttpError>
    where
        B: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| FederationHttpError::InvalidEndpoint)?;
        let mut request = self
            .client
            .request(method, url)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(credential) = node_credential {
            request = request.bearer_auth(credential);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| FederationHttpError::TransportUnavailable)?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FEDERATION_RESPONSE_BYTES as u64)
        {
            return Err(FederationHttpError::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| FederationHttpError::TransportUnavailable)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_FEDERATION_RESPONSE_BYTES {
                return Err(FederationHttpError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| FederationHttpError::InvalidResponse)
    }
}

fn validate_keeper_endpoint(value: &str) -> Result<Url, FederationHttpError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_KEEPER_ENDPOINT_BYTES {
        return Err(FederationHttpError::InvalidEndpoint);
    }
    let mut url = Url::parse(value).map_err(|_| FederationHttpError::InvalidEndpoint)?;
    let loopback_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
    if (url.scheme() != "https" && !loopback_http)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(FederationHttpError::InvalidEndpoint);
    }
    if !url.path().ends_with('/') {
        let mut path = url.path().to_owned();
        path.push('/');
        url.set_path(&path);
    }
    Ok(url)
}

fn status_error(status: StatusCode) -> FederationHttpError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            FederationHttpError::AuthenticationRejected
        }
        StatusCode::CONFLICT => FederationHttpError::Conflict,
        other => FederationHttpError::RemoteRejected(other.as_u16()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Json, Router, body::Body, http::header, response::Redirect, routing::get};
    use serde_json::json;
    use swarm_domain::{
        ApiaryId, ApiaryInvitationId, ApiaryTaskId, FederationClaimState,
        FederationJoinSubmissionPayload, FederationMembershipReceipt,
        FederationMembershipReceiptId, FederationMembershipReceiptPayload, FederationNodeId,
        FederationTaskCommandId, FederationTaskCommandKind, FederationTaskCommandOutcome, HiveId,
        OperatorId, SharedWorkBackend, TaskState,
    };

    use super::*;

    #[test]
    fn endpoint_validation_requires_secure_secret_free_urls() {
        for invalid in [
            "",
            "http://keeper.example.test",
            "https://user:secret@keeper.example.test",
            "https://keeper.example.test?secret=yes",
            "https://keeper.example.test/#fragment",
        ] {
            assert!(matches!(
                FederationHttpClient::new(invalid),
                Err(FederationHttpError::InvalidEndpoint)
            ));
        }
        assert!(FederationHttpClient::new("https://keeper.example.test/swarm").is_ok());
        assert!(FederationHttpClient::new("http://127.0.0.1:8766").is_ok());
    }

    #[tokio::test]
    async fn join_transport_delivers_the_sealed_submission_only_to_the_bounded_endpoint() {
        let submission = sample_join_submission();
        let expected_submission = submission.clone();
        let acceptance = sample_join_acceptance(&submission);
        let expected_acceptance = acceptance.clone();
        let app = Router::new().route(
            "/swarm/api/v1/federation/join",
            axum::routing::post(
                move |headers: axum::http::HeaderMap,
                      Json(body): Json<FederationJoinSubmission>| {
                    let expected_submission = expected_submission.clone();
                    let expected_acceptance = expected_acceptance.clone();
                    async move {
                        assert!(headers.get(header::AUTHORIZATION).is_none());
                        assert_eq!(body, expected_submission);
                        Json(expected_acceptance)
                    }
                },
            ),
        );
        let address = spawn_server(app).await;
        let client = FederationHttpClient::new(&format!("http://{address}/swarm")).unwrap();

        assert_eq!(client.join(&submission).await.unwrap(), acceptance);
    }

    #[tokio::test]
    async fn claim_transport_preserves_prefix_auth_and_typed_conflicts() {
        let claim = sample_claim();
        let expected = claim.clone();
        let confirmed = claim.clone();
        let released = claim.clone();
        let app = Router::new()
            .route(
                "/swarm/api/v1/federation/claims",
                axum::routing::post(
                    |headers: axum::http::HeaderMap, Json(body): Json<serde_json::Value>| async move {
                        assert_eq!(headers[header::AUTHORIZATION], "Bearer member-secret");
                        assert_eq!(body["issue_id"], "20001");
                        Json(expected)
                    },
                ),
            )
            .route(
                "/swarm/api/v1/federation/claims/{claim_id}/confirmation",
                axum::routing::post(
                    move |headers: axum::http::HeaderMap,
                          axum::extract::Path(claim_id): axum::extract::Path<String>| {
                        let confirmed = confirmed.clone();
                        async move {
                            assert_eq!(headers[header::AUTHORIZATION], "Bearer member-secret");
                            assert_eq!(claim_id, confirmed.id.to_string());
                            Json(confirmed)
                        }
                    },
                ),
            )
            .route(
                "/swarm/api/v1/federation/claims/{claim_id}",
                axum::routing::delete(
                    move |headers: axum::http::HeaderMap,
                          axum::extract::Path(claim_id): axum::extract::Path<String>| {
                        let released = released.clone();
                        async move {
                            assert_eq!(headers[header::AUTHORIZATION], "Bearer member-secret");
                            assert_eq!(claim_id, released.id.to_string());
                            Json(released)
                        }
                    },
                ),
            )
            .route(
                "/swarm/api/v1/federation/claims/conflict",
                get(|| async { StatusCode::CONFLICT }),
            );
        let address = spawn_server(app).await;
        let client = FederationHttpClient::new(&format!("http://{address}/swarm")).unwrap();
        let received = client
            .reserve_claim("member-secret", "10001", "20001", "WWD-101")
            .await
            .unwrap();
        assert_eq!(received, claim);
        assert_eq!(
            client
                .confirm_claim("member-secret", claim.id)
                .await
                .unwrap(),
            claim
        );
        assert_eq!(
            client
                .release_claim("member-secret", claim.id)
                .await
                .unwrap(),
            claim
        );

        let error = client
            .send_json::<(), serde_json::Value>(
                Method::GET,
                "api/v1/federation/claims/conflict",
                Some("member-secret"),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, FederationHttpError::Conflict));
    }

    #[tokio::test]
    async fn task_command_transport_preserves_identity_and_receipt() {
        let command = FederationTaskCommand {
            id: FederationTaskCommandId::new(),
            apiary_id: ApiaryId::new(),
            task_id: ApiaryTaskId::new(),
            expected_revision: 4,
            kind: FederationTaskCommandKind::Transition,
            target_state: Some(TaskState::Active),
            created_at: 1_000,
        };
        let receipt = FederationTaskCommandReceipt {
            command_id: command.id,
            outcome: FederationTaskCommandOutcome::Applied,
            task_revision: Some(5),
            processed_at: 1_001,
        };
        let expected_command = command.clone();
        let expected_receipt = receipt.clone();
        let app = Router::new().route(
            "/swarm/api/v1/federation/tasks/commands",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, Json(body): Json<FederationTaskCommand>| {
                    let expected_command = expected_command.clone();
                    let expected_receipt = expected_receipt.clone();
                    async move {
                        assert_eq!(headers[header::AUTHORIZATION], "Bearer member-secret");
                        assert_eq!(body, expected_command);
                        Json(expected_receipt)
                    }
                },
            ),
        );
        let address = spawn_server(app).await;
        let client = FederationHttpClient::new(&format!("http://{address}/swarm")).unwrap();
        assert_eq!(
            client
                .submit_task_command("member-secret", &command)
                .await
                .unwrap(),
            receipt
        );
    }

    #[tokio::test]
    async fn redirects_and_oversized_responses_fail_closed() {
        let redirected_requests = Arc::new(AtomicUsize::new(0));
        let counter = redirected_requests.clone();
        let oversized = "x".repeat(MAX_FEDERATION_RESPONSE_BYTES + 1);
        let app = Router::new()
            .route(
                "/swarm/api/v1/federation/catalog",
                get(|| async { Redirect::temporary("/poison") }),
            )
            .route(
                "/poison",
                get(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Json(json!({}))
                    }
                }),
            )
            .route("/large", get(move || async move { Body::from(oversized) }));
        let address = spawn_server(app).await;
        let client = FederationHttpClient::new(&format!("http://{address}/swarm")).unwrap();
        assert!(matches!(
            client.catalog("member-secret").await,
            Err(FederationHttpError::RemoteRejected(307))
        ));
        assert_eq!(redirected_requests.load(Ordering::SeqCst), 0);
        assert!(matches!(
            client
                .send_json::<(), serde_json::Value>(Method::GET, "../large", None, None)
                .await,
            Err(FederationHttpError::ResponseTooLarge)
        ));
    }

    async fn spawn_server(app: Router) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        address
    }

    fn sample_claim() -> FederationSharedClaim {
        FederationSharedClaim {
            id: FederationClaimId::new(),
            apiary_id: ApiaryId::new(),
            project_id: "10001".into(),
            issue_id: "20001".into(),
            issue_key: "WWD-101".into(),
            home_node_id: FederationNodeId::new(),
            home_hive_id: HiveId::new(),
            home_operator_id: OperatorId::new(),
            state: FederationClaimState::Reserved,
            reserved_at: 1_000,
            reservation_expires_at: 1_120,
            confirmed_at: None,
            released_at: None,
        }
    }

    fn sample_join_submission() -> FederationJoinSubmission {
        FederationJoinSubmission {
            payload: FederationJoinSubmissionPayload {
                schema_version: 1,
                protocol_version: 1,
                invitation_id: ApiaryInvitationId::new(),
                apiary_id: ApiaryId::new(),
                required_policy_revision: 3,
                promoted_project_catalog_digest: "catalog-digest".into(),
                invited_node_id: FederationNodeId::new(),
                invited_hive_id: HiveId::new(),
                invited_operator_id: OperatorId::new(),
                submitted_at: 1_000,
            },
            signature: "member-signature".into(),
            one_time_secret: "one-time-secret".into(),
        }
    }

    fn sample_join_acceptance(submission: &FederationJoinSubmission) -> FederationJoinAcceptance {
        FederationJoinAcceptance {
            receipt: FederationMembershipReceipt {
                payload: FederationMembershipReceiptPayload {
                    schema_version: 1,
                    protocol_version: 1,
                    receipt_id: FederationMembershipReceiptId::new(),
                    invitation_id: submission.payload.invitation_id,
                    apiary_id: submission.payload.apiary_id,
                    apiary_name: "Wildflower Garden".into(),
                    shared_work_backend: SharedWorkBackend::Jira,
                    policy_revision: submission.payload.required_policy_revision,
                    promoted_project_catalog_digest: submission
                        .payload
                        .promoted_project_catalog_digest
                        .clone(),
                    keeper_node_id: FederationNodeId::new(),
                    keeper_hive_id: HiveId::new(),
                    keeper_operator_id: OperatorId::new(),
                    member_node_id: submission.payload.invited_node_id,
                    member_hive_id: submission.payload.invited_hive_id,
                    member_operator_id: submission.payload.invited_operator_id,
                    joined_at: 1_001,
                    credential_expires_at: 2_000,
                },
                signature: "keeper-signature".into(),
            },
            node_credential: "member-node-credential".into(),
        }
    }
}
