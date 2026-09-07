//! Explicit operator-reviewed Hive feedback, separate from central support and task intake.
use swarm_domain::{SupportDeliveryState, SupportSubmissionInput};
use swarm_persistence::{
    SupportOutboxEntry, SupportOutboxError, SupportOutboxStatus, SupportReceipt, TaskStore,
};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

/// A deployment-approved HTTPS origin, never a caller-selected upload URL.
#[derive(Clone)]
pub struct SupportDestination(String);

#[derive(Debug, Error)]
pub enum HiveSupportServiceError {
    #[error(
        "support destination must be an HTTPS origin without credentials, path, query or fragment"
    )]
    InvalidDestination,
    #[error(transparent)]
    InvalidSubmission(#[from] swarm_domain::SupportValidationError),
    #[error(transparent)]
    Outbox(#[from] SupportOutboxError),
}

impl SupportDestination {
    /// Validates deployment configuration; this does not authorize an arbitrary browser URL.
    ///
    /// # Errors
    /// Refuses ambiguous URL syntax, plaintext transport and non-origin input.
    pub fn parse(origin: &str) -> Result<Self, HiveSupportServiceError> {
        // Do not let URL normalization erase userinfo or a caller-supplied path.
        let authority = origin.strip_prefix("https://").unwrap_or("");
        let authority = authority.strip_suffix('/').unwrap_or(authority);
        if origin.len() > 2048
            || authority.is_empty()
            || authority.contains(['/', '@', '?', '#'])
            || origin
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            || origin.contains('\\')
        {
            return Err(HiveSupportServiceError::InvalidDestination);
        }
        let url = Url::parse(origin).map_err(|_| HiveSupportServiceError::InvalidDestination)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(HiveSupportServiceError::InvalidDestination);
        }
        Ok(Self(format!(
            "{}/api/support/v1/submissions",
            url.origin().ascii_serialization()
        )))
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.0
    }
}

/// Adapters must authenticate the local operator before accepting reviewed input.
/// Construction, listing and saving never start network activity or export old reports.
#[derive(Clone)]
pub struct HiveSupportService {
    store: TaskStore,
    destination: SupportDestination,
}

impl HiveSupportService {
    #[must_use]
    pub const fn new(store: TaskStore, destination: SupportDestination) -> Self {
        Self { store, destination }
    }

    /// Saves only the explicit reviewed payload and the deployment-owned destination.
    ///
    /// # Errors
    /// Invalid content, changed-key replays and capacity failures preserve existing reports.
    pub fn submit_reviewed(
        &self,
        input: SupportSubmissionInput,
        now: i64,
    ) -> Result<SupportOutboxEntry, HiveSupportServiceError> {
        Ok(self.store.enqueue_support_submission(
            &input.validate()?,
            self.destination.endpoint(),
            now,
        )?)
    }

    /// Content-free delivery status; a local save is not delivery confirmation.
    ///
    /// # Errors
    /// Propagates unreadable storage instead of reporting an empty outbox.
    pub fn statuses(&self) -> Result<Vec<SupportOutboxStatus>, HiveSupportServiceError> {
        Ok(self.store.support_submission_statuses()?)
    }

    /// Sole process-owned sender claims the exact saved report, not current UI content.
    ///
    /// # Errors
    /// Refuses changed destinations and exhausted, completed or concurrent attempts.
    pub fn claim(
        &self,
        key: Uuid,
        now: i64,
    ) -> Result<SupportOutboxEntry, HiveSupportServiceError> {
        Ok(self.store.claim_support_submission_for_destination(
            key,
            self.destination.endpoint(),
            now,
        )?)
    }

    /// Fences an untrusted transport receipt; invalid or missing receipts stay uncertain.
    ///
    /// # Errors
    /// Superseded attempts and storage failures cannot settle another owner's claim.
    pub fn settle(
        &self,
        key: Uuid,
        attempt: Uuid,
        receipt: Option<SupportReceipt>,
        now: i64,
    ) -> Result<(), HiveSupportServiceError> {
        if let Some(receipt) = receipt {
            match self.store.settle_support_submission(
                key,
                attempt,
                SupportDeliveryState::Confirmed,
                Some(receipt),
                now,
            ) {
                Ok(()) => return Ok(()),
                Err(SupportOutboxError::InvalidTransition) => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(self.store.settle_support_submission(
            key,
            attempt,
            SupportDeliveryState::Uncertain,
            None,
            now,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{SupportDeliveryState, SupportKind};

    fn input() -> SupportSubmissionInput {
        SupportSubmissionInput {
            submission_key: Uuid::from_u128(7),
            kind: SupportKind::BugReport,
            email: "fictional@example.invalid".into(),
            name: None,
            subject: "Fictional feedback".into(),
            body: "Reviewed text only".into(),
        }
    }

    #[test]
    fn destination_accepts_only_https_origins_and_owns_the_fixed_route() {
        for invalid in [
            "http://support.example.invalid",
            "https://user:secret@support.example.invalid",
            "https://support.example.invalid/path",
            "https://support.example.invalid/?token=x",
            "https://support.example.invalid/#fragment",
            "https://support.example.invalid\\other",
            " https://support.example.invalid",
            "https://support.example.invalid\n",
            "https://@support.example.invalid",
            "https://support.example.invalid/a/..",
            "https:////support.example.invalid",
        ] {
            assert!(SupportDestination::parse(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(
            SupportDestination::parse("https://SUPPORT.example.invalid:443/")
                .unwrap()
                .endpoint(),
            "https://support.example.invalid/api/support/v1/submissions"
        );
    }

    #[test]
    fn configuration_change_cannot_redirect_saved_content_or_consume_retry_budget() {
        let store = TaskStore::in_memory().unwrap();
        let original = HiveSupportService::new(
            store.clone(),
            SupportDestination::parse("https://original.example.invalid").unwrap(),
        );
        let saved = original.submit_reviewed(input(), 1).unwrap();
        assert_eq!(saved.delivery.state, SupportDeliveryState::Pending);
        let changed = HiveSupportService::new(
            store.clone(),
            SupportDestination::parse("https://different.example.invalid").unwrap(),
        );
        assert!(matches!(
            changed.claim(saved.submission_key, 2),
            Err(HiveSupportServiceError::Outbox(
                SupportOutboxError::Conflict
            ))
        ));
        assert!(matches!(
            changed.submit_reviewed(input(), 2),
            Err(HiveSupportServiceError::Outbox(
                SupportOutboxError::Conflict
            ))
        ));
        let status = changed.statuses().unwrap().remove(0);
        assert_eq!(status.delivery.attempts, 0);
        assert_eq!(status.delivery.state, SupportDeliveryState::Pending);
        let claimed = original.claim(saved.submission_key, 3).unwrap();
        assert_eq!(claimed.frozen_submission, saved.frozen_submission);
        assert_eq!(claimed.destination, saved.destination);
        assert_eq!(claimed.delivery.attempts, 1);
        assert!(original.claim(saved.submission_key, 4).is_err());
    }

    #[test]
    fn wrong_receipt_stays_uncertain_and_cannot_settle_a_newer_attempt() {
        let store = TaskStore::in_memory().unwrap();
        let service = HiveSupportService::new(
            store.clone(),
            SupportDestination::parse("https://support.example.invalid").unwrap(),
        );
        let saved = service.submit_reviewed(input(), 1).unwrap();
        let first = service.claim(saved.submission_key, 2).unwrap();
        let first_id = first.delivery.attempt_id.unwrap();
        let wrong = SupportReceipt {
            submission_key: Uuid::from_u128(99).to_string(),
            conversation_id: Uuid::from_u128(2).to_string(),
            message_id: Uuid::from_u128(3).to_string(),
            created_at: 3,
            deduplicated: false,
        };
        service
            .settle(saved.submission_key, first_id, Some(wrong), 3)
            .unwrap();
        assert_eq!(
            service.statuses().unwrap()[0].delivery.state,
            SupportDeliveryState::Uncertain
        );
        let second = service.claim(saved.submission_key, 4).unwrap();
        assert!(matches!(
            service.settle(saved.submission_key, first_id, None, 5),
            Err(HiveSupportServiceError::Outbox(
                SupportOutboxError::StaleAttempt
            ))
        ));
        assert_eq!(
            service.statuses().unwrap()[0].delivery.attempt_id,
            second.delivery.attempt_id
        );
        assert_eq!(second.frozen_submission, saved.frozen_submission);
    }
}
