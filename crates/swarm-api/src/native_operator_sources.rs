//! Private engine transport. Source retention does not answer an operator decision.
use std::{sync::Arc, time::Duration};
use swarm_application::TaskService;
use swarm_domain::MAX_NATIVE_INTERVIEW_BATCH;
use swarm_terminal::{HostClient, HostRequest, HostResponse, PROTOCOL_VERSION};
use tokio::sync::Semaphore;

#[derive(Debug, Default)]
struct IntakeReport {
    stored: usize,
    acknowledged: usize,
    unconfirmed: usize,
    refused: usize,
}

#[derive(Debug, Eq, PartialEq)]
enum IntakeError {
    Busy,
    Unsupported,
    Unavailable,
    Malformed,
    Deadline,
}

pub(super) async fn collect(state: &crate::AppState) {
    let (Some(client), Ok(service)) = (&state.terminal_host, crate::task_service(state)) else {
        return;
    };
    match collect_once(
        client,
        service,
        state.native_source_admission.clone(),
        crate::unix_timestamp(),
        Duration::from_secs(3),
    )
    .await
    {
        Ok(report) if report.unconfirmed > 0 => tracing::warn!(
            stored = report.stored,
            acknowledged = report.acknowledged,
            unconfirmed = report.unconfirmed,
            refused = report.refused,
            "native answer source retention or acknowledgement requires recovery"
        ),
        Ok(_) | Err(IntakeError::Busy | IntakeError::Unsupported) => {}
        Err(reason) => tracing::warn!(
            ?reason,
            "native answer source retention is unavailable; no answer was replayed"
        ),
    }
}

async fn collect_once(
    client: &HostClient,
    service: TaskService,
    admission: Arc<Semaphore>,
    now: i64,
    budget: Duration,
) -> Result<IntakeReport, IntakeError> {
    let permit = admission
        .try_acquire_owned()
        .map_err(|_| IntakeError::Busy)?;
    tokio::time::timeout(budget, async move {
        match client
            .request(&HostRequest::Ping)
            .await
            .map_err(|_| IntakeError::Unavailable)?
        {
            HostResponse::Pong { protocol_version } if protocol_version == PROTOCOL_VERSION => {}
            HostResponse::Pong { .. } => return Err(IntakeError::Unsupported),
            _ => return Err(IntakeError::Malformed),
        }
        let HostResponse::NativeInterviews { entries } = client
            .request(&HostRequest::ReadNativeInterviews)
            .await
            .map_err(|_| IntakeError::Unavailable)?
        else {
            return Err(IntakeError::Malformed);
        };
        if entries.len() > MAX_NATIVE_INTERVIEW_BATCH {
            return Err(IntakeError::Malformed);
        }
        let count = entries.len();
        if count == 0 {
            return Ok(IntakeReport::default());
        }
        // Keep the sole admission permit inside the blocking job and return it
        // with its result. Cancellation cannot create another database job while
        // the previous save is still finishing; a late commit remains retry-safe.
        let (saved, _permit) = tokio::task::spawn_blocking(move || {
            (service.retain_native_sources(&entries, now), permit)
        })
        .await
        .map_err(|_| IntakeError::Unavailable)?;
        let mut report = IntakeReport {
            stored: saved.newly_stored,
            refused: saved.refused,
            unconfirmed: count,
            ..IntakeReport::default()
        };
        for receipt in saved.receipts {
            match client
                .request(&HostRequest::AcknowledgeNativeInterview { id: receipt.id() })
                .await
            {
                Ok(HostResponse::Acknowledged) => {
                    report.acknowledged += 1;
                    report.unconfirmed -= 1;
                }
                // The engine may already have accepted an acknowledgement whose
                // reply was lost. The source is durable either way. Never resend
                // terminal input or claim that this source answered a decision.
                _ => break,
            }
        }
        Ok(report)
    })
    .await
    .map_err(|_| IntakeError::Deadline)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use swarm_domain::{
        NativeInterviewEvidence, NativeInterviewOption, NativeInterviewQuestion,
        OperatorSubmissionId, PresenceDeviceId, ProviderConversationId, WorkerSessionId,
    };
    use swarm_persistence::TaskStore;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
        sync::Notify,
        task::JoinHandle,
    };

    fn source(store: &TaskStore) -> NativeInterviewEvidence {
        let worker = store.ensure_queen("/fictional").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        NativeInterviewEvidence {
            id: OperatorSubmissionId::new(),
            session_id: session,
            conversation: ProviderConversationId::new(),
            selection_revision: 1,
            tool_use_id: "toolu_fictional".into(),
            devices: vec![PresenceDeviceId::new()],
            first_write_sequence: 1,
            submit_sequence: 2,
            questions: vec![NativeInterviewQuestion {
                question: "Which jar?".into(),
                header: "Jar".into(),
                options: vec![
                    NativeInterviewOption {
                        label: "Amber".into(),
                        description: "Keep closed".into(),
                    },
                    NativeInterviewOption {
                        label: "Blue".into(),
                        description: "Do not move".into(),
                    },
                ],
                multi_select: false,
            }],
            answers: [("Which jar?".into(), "Amber".into())].into(),
        }
    }

    struct FakeEngine {
        _directory: tempfile::TempDir,
        client: HostClient,
        server: JoinHandle<()>,
        entries: Arc<Mutex<Vec<NativeInterviewEvidence>>>,
        reads: Arc<AtomicUsize>,
        read_started: Arc<Notify>,
    }

    impl FakeEngine {
        fn start(
            store: TaskStore,
            entries: Vec<NativeInterviewEvidence>,
            protocol_version: u16,
            lose_first_ack: Option<bool>,
            hold_read: bool,
        ) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("fixture.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let entries = Arc::new(Mutex::new(entries));
            let reads = Arc::new(AtomicUsize::new(0));
            let read_started = Arc::new(Notify::new());
            let kept = entries.clone();
            let read_count = reads.clone();
            let started = read_started.clone();
            let server = tokio::spawn(async move {
                let mut lose_ack = lose_first_ack;
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut line = String::new();
                    BufReader::new(&mut socket)
                        .read_line(&mut line)
                        .await
                        .unwrap();
                    let request: HostRequest = serde_json::from_str(&line).unwrap();
                    let response = match request {
                        HostRequest::Ping => HostResponse::Pong { protocol_version },
                        HostRequest::ReadNativeInterviews => {
                            read_count.fetch_add(1, Ordering::SeqCst);
                            started.notify_one();
                            if hold_read {
                                std::future::pending::<()>().await;
                            }
                            HostResponse::NativeInterviews {
                                entries: kept.lock().unwrap().clone(),
                            }
                        }
                        HostRequest::AcknowledgeNativeInterview { id } => {
                            // This is the critical order assertion: even the first
                            // lost acknowledgement must follow a committed save.
                            assert!(store.native_interview(id).unwrap().is_some());
                            if let Some(remove_source) = lose_ack.take() {
                                if remove_source {
                                    kept.lock().unwrap().retain(|entry| entry.id != id);
                                }
                                continue;
                            }
                            kept.lock().unwrap().retain(|entry| entry.id != id);
                            HostResponse::Acknowledged
                        }
                        other => panic!("unexpected fixture request: {other:?}"),
                    };
                    let mut bytes = serde_json::to_vec(&response).unwrap();
                    bytes.push(b'\n');
                    socket.write_all(&bytes).await.unwrap();
                }
            });
            Self {
                _directory: directory,
                client: HostClient::new(path),
                server,
                entries,
                reads,
                read_started,
            }
        }

        async fn stop(self) {
            self.server.abort();
            assert!(
                self.server.await.unwrap_err().is_cancelled(),
                "fixture must not have panicked"
            );
        }
    }

    #[tokio::test]
    async fn lost_acknowledgement_and_api_replacement_do_not_lose_or_replay_an_answer() {
        // Exercise both a request not applied and an applied acknowledgement
        // whose response was lost. Both must leave the source durable.
        for remove_source in [false, true] {
            let store = TaskStore::in_memory().unwrap();
            let evidence = source(&store);
            let engine = FakeEngine::start(
                store.clone(),
                vec![evidence.clone()],
                PROTOCOL_VERSION,
                Some(remove_source),
                false,
            );
            let first = collect_once(
                &engine.client,
                TaskService::new(store.clone()),
                Arc::new(Semaphore::new(1)),
                100,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
            assert_eq!(first.stored, 1);
            assert_eq!(first.acknowledged, 0);
            assert_eq!(first.unconfirmed, 1);
            assert_eq!(
                store.native_interview(evidence.id).unwrap().unwrap().source,
                evidence
            );
            // A replacement API/service starts with no in-memory admission state.
            let second = collect_once(
                &engine.client,
                TaskService::new(store.clone()),
                Arc::new(Semaphore::new(1)),
                101,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
            assert_eq!(second.stored, 0);
            assert_eq!(second.acknowledged, usize::from(!remove_source));
            assert!(engine.entries.lock().unwrap().is_empty());
            assert!(store.list_decision_requests().unwrap().is_empty());
            assert!(store.claim_decision_deliveries(102).unwrap().is_empty());
            engine.stop().await;
        }
    }

    #[tokio::test]
    async fn invalid_source_stays_unacknowledged_while_valid_source_is_saved() {
        let store = TaskStore::in_memory().unwrap();
        let evidence = source(&store);
        let mut invalid = evidence.clone();
        invalid.id = OperatorSubmissionId::new();
        invalid.session_id = WorkerSessionId::new();
        let engine = FakeEngine::start(
            store.clone(),
            vec![invalid.clone(), evidence],
            PROTOCOL_VERSION,
            None,
            false,
        );
        let result = collect_once(
            &engine.client,
            TaskService::new(store.clone()),
            Arc::new(Semaphore::new(1)),
            100,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(result.refused, 1);
        assert_eq!(result.unconfirmed, 1);
        assert_eq!(result.acknowledged, 1);
        assert!(store.native_interview(invalid.id).unwrap().is_none());
        assert_eq!(engine.entries.lock().unwrap().len(), 1);
        engine.stop().await;
    }

    #[tokio::test]
    async fn protocol16_receives_only_ping_and_oversized_batches_are_not_saved() {
        let store = TaskStore::in_memory().unwrap();
        let evidence = source(&store);
        let engine = FakeEngine::start(store.clone(), vec![evidence.clone()], 16, None, false);
        let result = collect_once(
            &engine.client,
            TaskService::new(store.clone()),
            Arc::new(Semaphore::new(1)),
            100,
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(result.unwrap_err(), IntakeError::Unsupported);
        assert_eq!(engine.reads.load(Ordering::SeqCst), 0);
        engine.stop().await;
        let engine = FakeEngine::start(
            store.clone(),
            vec![evidence.clone(); MAX_NATIVE_INTERVIEW_BATCH + 1],
            PROTOCOL_VERSION,
            None,
            false,
        );
        let result = collect_once(
            &engine.client,
            TaskService::new(store.clone()),
            Arc::new(Semaphore::new(1)),
            100,
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(result.unwrap_err(), IntakeError::Malformed);
        assert!(store.native_interview(evidence.id).unwrap().is_none());
        engine.stop().await;
    }

    #[tokio::test]
    async fn hung_read_has_one_owned_pass_and_a_bounded_deadline() {
        let store = TaskStore::in_memory().unwrap();
        let evidence = source(&store);
        let engine = FakeEngine::start(
            store.clone(),
            vec![evidence.clone()],
            PROTOCOL_VERSION,
            None,
            true,
        );
        let admission = Arc::new(Semaphore::new(1));
        let client = engine.client.clone();
        let service = TaskService::new(store.clone());
        let admitted = admission.clone();
        let running = tokio::spawn(async move {
            collect_once(&client, service, admitted, 100, Duration::from_secs(3)).await
        });
        tokio::time::timeout(Duration::from_secs(2), engine.read_started.notified())
            .await
            .unwrap();
        let overlap = collect_once(
            &engine.client,
            TaskService::new(store.clone()),
            admission.clone(),
            100,
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(overlap.unwrap_err(), IntakeError::Busy);
        assert_eq!(running.await.unwrap().unwrap_err(), IntakeError::Deadline);
        assert_eq!(engine.reads.load(Ordering::SeqCst), 1);
        assert_eq!(admission.available_permits(), 1);
        assert!(store.native_interview(evidence.id).unwrap().is_none());
        engine.stop().await;
    }
}
