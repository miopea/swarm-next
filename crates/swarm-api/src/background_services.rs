//! Owned periodic services: stop admission, finish the current pass, then join.
use std::{future::Future, time::Duration};
use tokio::{sync::watch, task::JoinSet};

pub(super) struct BackgroundServices {
    stop: watch::Sender<bool>,
    tasks: JoinSet<()>,
}

impl BackgroundServices {
    pub(super) fn new() -> Self {
        let (stop, _) = watch::channel(false);
        Self {
            stop,
            tasks: JoinSet::new(),
        }
    }

    pub(super) fn stop_signal(&self) -> watch::Sender<bool> {
        self.stop.clone()
    }

    pub(super) fn periodic<F, Fut>(&mut self, period: Duration, immediate: bool, mut run: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        assert!(
            self.tasks.len() < 6,
            "all background services need a bounded owner"
        );
        let mut stop = self.stop.subscribe();
        self.tasks.spawn(async move {
            let mut interval = tokio::time::interval(period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            if !immediate {
                interval.tick().await;
            }
            loop {
                if *stop.borrow() {
                    break;
                }
                tokio::select! {
                    biased;
                    _ = stop.changed() => break,
                    _ = interval.tick() => {}
                }
                if *stop.borrow() {
                    break;
                }
                // Do not select cancellation against a partially written delivery.
                run().await;
            }
        });
    }

    pub(super) async fn shutdown(self) {
        self.shutdown_with_budget(Duration::from_secs(45)).await;
    }

    async fn shutdown_with_budget(mut self, budget: Duration) {
        self.stop.send_replace(true);
        let finished = tokio::time::timeout(budget, async {
            while let Some(result) = self.tasks.join_next().await {
                if let Err(error) = result {
                    tracing::warn!(%error, "background service did not finish cleanly");
                }
            }
        })
        .await;
        if finished.is_err() {
            tracing::warn!(
                remaining = self.tasks.len(),
                "background shutdown budget exhausted; unfinished deliveries require existing uncertain recovery"
            );
            self.tasks.abort_all();
            while self.tasks.join_next().await.is_some() {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn shutdown_after_real_pty_paste_finishes_enter_and_durable_delivery() {
        use swarm_domain::{ProviderKind, TaskDispatchState, TaskState};
        use swarm_terminal::{
            HostClient, HostRequest, HostResponse, JournalLimits, ProviderCommand, SessionRegistry,
            TerminalSize,
        };
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(64 * 1024, 256), 1, [workspace.clone()])
                .unwrap(),
        );
        let session = registry.spawn(&ProviderCommand {
            executable: "/bin/sh".into(),
            arguments: vec!["-c".into(), "printf '❯ \\nauto mode on\\n'; while IFS= read -r line; do printf 'CONSUMED:%s\\n❯ \\nauto mode on\\n' \"$line\"; done".into()],
            working_directory: workspace.clone(),
        }, TerminalSize::new(24, 240)).unwrap();
        let socket = workspace.join("host.sock");
        let host = swarm_terminal_host::HostServer::bind(&socket, registry).unwrap();
        let server = tokio::spawn(host.run());
        let client = HostClient::new(socket);
        let session_id = session.id();
        let read_client = client.clone();
        let read = || async {
            match read_client
                .request(&HostRequest::Read {
                    session_id,
                    after_sequence: None,
                })
                .await
                .unwrap()
            {
                HostResponse::Output {
                    resume: swarm_terminal::Resume::Snapshot { snapshot },
                    ..
                } => swarm_terminal::snapshot_plain_text(
                    &snapshot.bytes,
                    snapshot.rows,
                    snapshot.columns,
                ),
                other => panic!("expected snapshot: {other:?}"),
            }
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            while !read().await.contains("auto mode on") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Shutdown fixture",
                ProviderKind::ClaudeCode,
                &workspace.to_string_lossy(),
                false,
                1,
            )
            .unwrap();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let task = store
            .create_task("SHUTDOWN-PASTE-FIXTURE", &workspace.to_string_lossy())
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, session_id).unwrap();
        let state = swarm_api::AppState::default()
            .with_terminal_host(client, "fixture-only")
            .with_task_store(store.clone());
        let mut services = BackgroundServices::new();
        services.periodic(Duration::from_secs(3600), true, move || {
            let state = state.clone();
            async move {
                state.supervise_workers().await;
            }
        });
        // Synchronize on actual PTY echo, not a sleep that merely hopes to hit
        // the interval between the paste write and Enter.
        let pasted = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let screen = read().await;
                if screen.contains("SHUTDOWN-PASTE-FIXTURE") {
                    break screen;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !pasted.contains("CONSUMED:"),
            "shutdown must begin before Enter"
        );
        services.shutdown().await;
        let consumed = read().await;
        assert_eq!(
            consumed.matches("CONSUMED:").count(),
            1,
            "exactly one Enter must reach the fixture: {consumed}"
        );
        assert_eq!(
            store.get_task(task.id).unwrap().dispatch_state,
            Some(TaskDispatchState::Delivered)
        );
        assert!(
            session.reader_running(),
            "API shutdown must not stop the terminal"
        );
        session.stop().unwrap();
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn shutdown_finishes_admitted_work_without_admitting_another_pass() {
        let mut services = BackgroundServices::new();
        let (started, observed) = tokio::sync::oneshot::channel();
        let (finish, allowed) = tokio::sync::oneshot::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let mut channels = Some((started, allowed));
        services.periodic(Duration::from_millis(1), true, move || {
            counted.fetch_add(1, Ordering::SeqCst);
            let (started, allowed) = channels.take().expect("no second pass during shutdown");
            async move {
                let _ = started.send(());
                let _ = allowed.await;
            }
        });
        observed.await.unwrap();
        services.stop_signal().send_replace(true);
        let shutdown = tokio::spawn(services.shutdown());
        tokio::task::yield_now().await;
        assert!(
            !shutdown.is_finished(),
            "in-flight work must not be dropped"
        );
        finish.send(()).unwrap();
        shutdown.await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shutdown_cancels_idle_timers_and_bounds_a_stuck_pass() {
        let mut idle = BackgroundServices::new();
        idle.periodic(Duration::from_secs(3600), false, || async {
            panic!("not due");
        });
        idle.shutdown().await;
        let mut stuck = BackgroundServices::new();
        let (started, observed) = tokio::sync::oneshot::channel();
        let (dropped, released) = tokio::sync::oneshot::channel::<()>();
        let mut channels = Some((started, dropped));
        stuck.periodic(Duration::from_secs(3600), true, move || {
            let (started, dropped) = channels.take().unwrap();
            async move {
                let _retained = dropped;
                let _ = started.send(());
                std::future::pending::<()>().await;
            }
        });
        observed.await.unwrap();
        stuck.shutdown_with_budget(Duration::from_millis(1)).await;
        assert!(
            released.await.is_err(),
            "timed-out pass must be aborted and joined"
        );
    }
}
