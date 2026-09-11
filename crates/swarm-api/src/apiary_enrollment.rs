//! Bounded transport adapter for the application's durable joining workflow.
use super::{
    ApiError, ApiaryService, AppState, ApplicationError, Arc, Deserialize, HeaderMap, IntoResponse,
    Json, Response, State, StatusCode, TaskStoreError, apiary_service, application_error,
    authorize, federation_http, federation_http_error, header, unix_timestamp,
};
use swarm_domain::{ApiaryEnrollment, ApiaryEnrollmentOffer};

#[derive(Deserialize)]
pub(super) struct SubmitEnrollment {
    offer: ApiaryEnrollmentOffer,
    secret: String,
}

pub(super) async fn submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SubmitEnrollment>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let record = apiary_service(&state)?
        .begin_consented_enrollment(&request.offer, &request.secret, unix_timestamp())
        .map_err(application_error)?;
    // The owned federation service performs transport even if this response
    // is lost or the browser closes. No detached task per submission.
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(record),
    )
        .into_response())
}

pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let records = apiary_service(&state)?
        .enrollments()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(records)).into_response())
}

impl AppState {
    /// The existing owned federation service calls this on startup and every
    /// reconciliation pass. One runner, at most four bounded attempts per pass.
    pub async fn reconcile_apiary_enrollments(&self) {
        let Ok(_guard) = self.enrollment_delivery.try_lock() else {
            return;
        };
        let Ok(service) = apiary_service(self) else {
            return;
        };
        let Ok(records) = service.pending_enrollments(unix_timestamp()) else {
            return;
        };
        for record in records {
            let problem = match reconcile_one(&service, &record, unix_timestamp()).await {
                Ok(()) => None,
                Err(error) => {
                    use swarm_domain::ApiaryEnrollmentProblem;
                    Some(match error.code {
                        "keeper_unavailable" => ApiaryEnrollmentProblem::KeeperUnavailable,
                        "apiary_invitation_rejected" => {
                            ApiaryEnrollmentProblem::InvitationUnavailable
                        }
                        "keeper_response_invalid" => ApiaryEnrollmentProblem::RuntimeIncompatible,
                        _ if error.status.is_server_error() => {
                            ApiaryEnrollmentProblem::KeeperUnavailable
                        }
                        _ => ApiaryEnrollmentProblem::ApprovalChanged,
                    })
                }
            };
            if let Err(error) =
                service.record_enrollment_attempt(record.consent.link_id, problem, unix_timestamp())
            {
                tracing::warn!(%error, "Apiary enrollment outcome could not be saved");
            }
        }
    }
}

async fn reconcile_one(
    service: &ApiaryService,
    record: &ApiaryEnrollment,
    now: i64,
) -> Result<(), ApiError> {
    let link_id = record.consent.link_id;
    let (endpoint, secret) = service
        .keeper_link_credential(link_id)
        .map_err(application_error)?;
    let client =
        federation_http::FederationHttpClient::new(&endpoint).map_err(enrollment_http_error)?;
    // Observe first: a lost introduction response may already have been
    // approved remotely. Re-presenting then would wrongly fail as resolved.
    let mut poll = client
        .bootstrap(link_id, &secret, None)
        .await
        .map_err(enrollment_http_error)?;
    if poll.link.state == swarm_domain::ApiaryJoinLinkState::Open {
        let card = service.connection_card(now).map_err(application_error)?;
        poll = client
            .bootstrap(link_id, &secret, Some(&card))
            .await
            .map_err(enrollment_http_error)?;
    }
    service
        .record_keeper_link_poll(&poll.link, now)
        .map_err(application_error)?;
    if matches!(
        poll.link.state,
        swarm_domain::ApiaryJoinLinkState::Revoked | swarm_domain::ApiaryJoinLinkState::Expired
    ) {
        return Err(ApiError::new(
            StatusCode::GONE,
            "apiary_invitation_rejected",
            "Invitation is no longer available",
        ));
    }
    let Some(invitation) = poll.invitation else {
        return Ok(());
    };
    match service.import_invitation(&invitation, now) {
        Ok(_) | Err(ApplicationError::Store(TaskStoreError::FederationInvitationConflict)) => {}
        Err(error) => return Err(application_error(error)),
    }
    let invitation_id = invitation.invitation.payload.invitation_id;
    service
        .prepare_consented_join(link_id, invitation_id, now)
        .map_err(application_error)?;
    let submission = service
        .prepare_imported_join_submission(
            invitation_id,
            swarm_domain::JiraConnectionState::NotConnected,
            now,
        )
        .map_err(application_error)?;
    let acceptance = client
        .join(&submission)
        .await
        .map_err(enrollment_http_error)?;
    service
        .apply_remote_join_acceptance(invitation_id, &acceptance, now)
        .map_err(application_error)?;
    service
        .finish_consented_join(link_id)
        .map_err(application_error)?;
    Ok(())
}

fn enrollment_http_error(error: federation_http::FederationHttpError) -> ApiError {
    match error {
        federation_http::FederationHttpError::RemoteRejected(status)
            if status == 429 || status >= 500 =>
        {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "keeper_unavailable",
                "Keeper is temporarily unavailable",
            )
        }
        other => federation_http_error(other),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{HostClient, SharedWorkBackend, TaskStore, router};
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use swarm_domain::ApiaryEnrollmentPhase;
    use tower::ServiceExt;

    #[tokio::test]
    async fn unavailable_keeper_persists_backoff_and_does_not_retry_early() {
        let now = unix_timestamp();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Fictional garden", SharedWorkBackend::Jira, now - 1)
            .unwrap();
        let bundle = keeper.issue_apiary_join_link(&endpoint, now, 3600).unwrap();
        let member = TaskStore::in_memory().unwrap();
        ApiaryService::new(member.clone())
            .begin_consented_enrollment(
                &bundle.enrollment_offer.unwrap(),
                &bundle.one_time_secret,
                now,
            )
            .unwrap();
        let state = AppState::default().with_task_store(member.clone());
        state.reconcile_apiary_enrollments().await;
        let failed = member.apiary_enrollments().unwrap().remove(0);
        assert_eq!(
            failed.problem,
            Some(swarm_domain::ApiaryEnrollmentProblem::KeeperUnavailable)
        );
        assert_eq!(failed.consecutive_failures, 1);
        assert!(failed.next_attempt_at.unwrap() > unix_timestamp());
        state.reconcile_apiary_enrollments().await;
        assert_eq!(member.apiary_enrollments().unwrap()[0], failed);
        assert!(
            member
                .local_hive_identity()
                .unwrap()
                .hive
                .apiary_id
                .is_none()
        );
    }

    #[tokio::test]
    async fn submitted_hive_joins_after_restart_without_browser_or_jira() {
        assert_background_join(false).await;
    }

    #[tokio::test]
    async fn saved_membership_receipt_finishes_after_restart_with_keeper_offline() {
        assert_background_join(true).await;
    }

    async fn assert_background_join(crash_after_receipt: bool) {
        let now = unix_timestamp();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Fictional garden", SharedWorkBackend::Jira, now - 1)
            .unwrap();
        let bundle = keeper.issue_apiary_join_link(&endpoint, now, 3600).unwrap();
        let keeper_app = router(AppState::default().with_task_store(keeper.clone()));
        let server = tokio::spawn(async move { axum::serve(listener, keeper_app).await });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("member.sqlite");
        let member = TaskStore::open(&path).unwrap();
        let state = AppState::default()
            .with_task_store(member.clone())
            .with_terminal_host(HostClient::new("/unused/terminal.sock"), "test-secret");
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/enrollments")
                    .header(header::AUTHORIZATION, "Bearer test-secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "offer": bundle.enrollment_offer,
                "secret": bundle.one_time_secret })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        state.reconcile_apiary_enrollments().await;
        keeper
            .approve_apiary_join_link(bundle.link.id, unix_timestamp())
            .unwrap();
        drop(state);
        drop(member);
        // No further member browser request. Restart the application against
        // the saved database and run its owned reconciliation service.
        let reopened = TaskStore::open(&path).unwrap();
        let resumed = AppState::default().with_task_store(reopened.clone());
        if crash_after_receipt {
            let service = ApiaryService::new(reopened.clone());
            let client = federation_http::FederationHttpClient::new(&endpoint).unwrap();
            let poll = client
                .bootstrap(bundle.link.id, &bundle.one_time_secret, None)
                .await
                .unwrap();
            let invitation = poll.invitation.unwrap();
            let invitation_id = invitation.invitation.payload.invitation_id;
            service
                .import_invitation(&invitation, unix_timestamp())
                .unwrap();
            service
                .prepare_consented_join(bundle.link.id, invitation_id, unix_timestamp())
                .unwrap();
            let submission = service
                .prepare_imported_join_submission(
                    invitation_id,
                    swarm_domain::JiraConnectionState::NotConnected,
                    unix_timestamp(),
                )
                .unwrap();
            let accepted = client.join(&submission).await.unwrap();
            service
                .apply_remote_join_acceptance(invitation_id, &accepted, unix_timestamp())
                .unwrap();
            // Simulate interruption after durable receipt application and before
            // journal completion. Recovery must not depend on Keeper availability.
            assert_eq!(
                reopened.apiary_enrollments().unwrap()[0].phase,
                ApiaryEnrollmentPhase::Joining
            );
            server.abort();
        }
        resumed.reconcile_apiary_enrollments().await;
        assert_eq!(
            reopened.local_hive_identity().unwrap().hive.apiary_id,
            Some(bundle.link.apiary_id)
        );
        assert_eq!(
            reopened.apiary_enrollments().unwrap()[0].phase,
            ApiaryEnrollmentPhase::Complete
        );
        // Repeated reconciliation is inert and does not need another approval.
        resumed.reconcile_apiary_enrollments().await;
        assert_eq!(reopened.apiary_enrollments().unwrap().len(), 1);
        server.abort();
        let _ = server.await;
    }
}
