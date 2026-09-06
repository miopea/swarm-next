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
