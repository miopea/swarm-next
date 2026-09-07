//! One process-owned support sender. Never spawned by a browser or HTTP request.
use crate::support_transport::{SupportTransport, SupportTransportResult};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use swarm_application::{HiveSupportService, HiveSupportServiceError};
use tokio::sync::{Notify, watch};

#[derive(Debug, thiserror::Error)]
pub enum SupportSenderError {
    #[error("support sender HTTP transport could not initialize")]
    TransportInitialization,
    #[error("support sender storage task ended unexpectedly")]
    StorageTask,
    #[error(transparent)]
    Service(#[from] HiveSupportServiceError),
}

/// Run exactly once per API process, after the predecessor process has ended.
/// The runtime must signal stop and await this future, never abort it while storage runs.
/// Storage failure ends this owner visibly instead of repeatedly spinning on corruption.
///
/// # Errors
/// Propagates durable recovery, claim and settlement failure to runtime supervision.
pub async fn run(
    service: HiveSupportService,
    stop: watch::Receiver<bool>,
    wake: Arc<Notify>,
) -> Result<(), SupportSenderError> {
    let transport = SupportTransport::new(service.destination())
        .map_err(|_| SupportSenderError::TransportInitialization)?;
    run_owned(service, transport, stop, wake).await
}

trait Transport: Send + Sync {
    fn send(&self, body: &str) -> impl std::future::Future<Output = SupportTransportResult> + Send;
}

impl Transport for SupportTransport {
    async fn send(&self, body: &str) -> SupportTransportResult {
        self.send(body).await
    }
}

async fn storage<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, HiveSupportServiceError> + Send + 'static,
) -> Result<T, SupportSenderError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| SupportSenderError::StorageTask)?
        .map_err(Into::into)
}

fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(i64::MAX)
}

async fn run_owned(
    service: HiveSupportService,
    transport: impl Transport,
    mut stop: watch::Receiver<bool>,
    wake: Arc<Notify>,
) -> Result<(), SupportSenderError> {
    if *stop.borrow() {
        return Ok(());
    }
    let recovery = service.clone();
    storage(move || recovery.recover_interrupted(now())).await?;
    // Cadence avoids network churn; durable state/attempt fences establish correctness.
    let mut cadence = tokio::time::interval(Duration::from_secs(30));
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() { return Ok(()); }
                continue;
            }
            () = wake.notified() => (),
            _ = cadence.tick() => (),
        }
        let snapshot = service.clone();
        let keys = storage(move || snapshot.retryable_keys()).await?;
        for key in keys {
            if *stop.borrow() || stop.has_changed().is_err() {
                return Ok(());
            }
            let claim = service.clone();
            let entry = match storage(move || claim.claim(key, now())).await {
                Ok(entry) => entry,
                // Destination changes hold the original report, never redirect it.
                Err(SupportSenderError::Service(HiveSupportServiceError::Outbox(
                    swarm_persistence::SupportOutboxError::Conflict
                    | swarm_persistence::SupportOutboxError::InvalidTransition,
                ))) => continue,
                Err(error) => return Err(error),
            };
            let attempt = entry
                .delivery
                .attempt_id
                .ok_or(SupportSenderError::StorageTask)?;
            // Finish this bounded network attempt and its durable settlement even on shutdown.
            let receipt = match transport.send(&entry.frozen_submission).await {
                SupportTransportResult::Receipt(receipt) => Some(receipt),
                SupportTransportResult::Uncertain => None,
            };
            let settlement = service.clone();
            storage(move || settlement.settle(key, attempt, receipt, now())).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{num::NonZeroU32, path::Path};
    use swarm_application::{SupportDestination, SupportService};
    use swarm_domain::{SupportDeliveryState, SupportKind, SupportSubmissionInput};
    use swarm_persistence::{SupportStore, TaskStore};

    struct FictionalTransport {
        central: SupportService,
        stop: watch::Sender<bool>,
        lose_response: bool,
    }

    impl Transport for FictionalTransport {
        async fn send(&self, body: &str) -> SupportTransportResult {
            let input = serde_json::from_str(body).unwrap();
            let receipt = self.central.submit(input, 10).unwrap();
            // Shutdown arrives after remote acceptance but before local settlement.
            self.stop.send_replace(true);
            if self.lose_response {
                SupportTransportResult::Uncertain
            } else {
                SupportTransportResult::Receipt(receipt)
            }
        }
    }

    fn fixture() -> (HiveSupportService, SupportService, SupportSubmissionInput) {
        let hive = HiveSupportService::new(
            TaskStore::in_memory().unwrap(),
            SupportDestination::parse("https://support.example.invalid").unwrap(),
        );
        let central = SupportService::new(
            SupportStore::open(Path::new(":memory:"), NonZeroU32::new(10).unwrap()).unwrap(),
        );
        let input = SupportSubmissionInput {
            submission_key: "00000000-0000-0000-0000-000000000007".parse().unwrap(),
            kind: SupportKind::BugReport,
            email: "fictional@example.invalid".into(),
            name: None,
            subject: "Fictional owner recovery".into(),
            body: "No customer data".into(),
        };
        (hive, central, input)
    }

    #[tokio::test]
    async fn shutdown_settles_lost_response_and_next_owner_recovers_same_conversation() {
        let (hive, central, input) = fixture();
        hive.submit_reviewed(input, 1).unwrap();
        for lose_response in [true, false] {
            let (stop, receiver) = watch::channel(false);
            let transport = FictionalTransport {
                central: central.clone(),
                stop,
                lose_response,
            };
            tokio::time::timeout(
                Duration::from_secs(5),
                run_owned(hive.clone(), transport, receiver, Arc::new(Notify::new())),
            )
            .await
            .unwrap()
            .unwrap();
            let status = hive.statuses().unwrap().remove(0);
            assert_eq!(
                status.delivery.state,
                if lose_response {
                    SupportDeliveryState::Uncertain
                } else {
                    SupportDeliveryState::Confirmed
                }
            );
        }
        let status = hive.statuses().unwrap().remove(0);
        assert_eq!(status.delivery.attempts, 2);
        assert!(status.delivery.receipt.unwrap().deduplicated);
        assert_eq!(
            central.conversations(None, 20).unwrap().conversations.len(),
            1
        );
        assert!(hive.retryable_keys().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stopped_owner_does_not_claim_or_send_saved_work() {
        let (hive, central, input) = fixture();
        hive.submit_reviewed(input, 1).unwrap();
        let (stop, receiver) = watch::channel(true);
        run_owned(
            hive.clone(),
            FictionalTransport {
                central: central.clone(),
                stop,
                lose_response: false,
            },
            receiver,
            Arc::new(Notify::new()),
        )
        .await
        .unwrap();
        assert_eq!(hive.statuses().unwrap()[0].delivery.attempts, 0);
        assert!(
            central
                .conversations(None, 20)
                .unwrap()
                .conversations
                .is_empty()
        );
    }
}
