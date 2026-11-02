//! When the next held-back delivery can go (ADR 0140).
//!
//! A delivery can be held back for a terminal's pacing cooldown. Since ADR 0138
//! an agent's read no longer runs a pass, so a delivery held that way waited
//! for the next call that changed something, or for the supervisor's tick. The
//! end of a cooldown is a timestamp the store already holds, so the supervisor
//! wakes at it instead.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Notify;

/// The earliest moment a pacing cooldown on a live session ends, as the last
/// coordination pass found it.
///
/// Owned by the supervisor's loop, which waits on it beside its tick and its
/// wakeup. Every pass sets it again from durable state, so it is never more
/// than one pass out of date, and it holds one value, not a queue.
#[derive(Debug, Default)]
pub struct CoordinationDeadline {
    at: Mutex<Option<i64>>,
    changed: Notify,
}

impl CoordinationDeadline {
    /// Replaces the deadline. `None` means no cooldown is running.
    pub fn set(&self, at: Option<i64>) {
        let mut current = self.at.lock().unwrap_or_else(PoisonError::into_inner);
        if *current != at {
            *current = at;
            drop(current);
            self.changed.notify_waiters();
        }
    }

    #[must_use]
    pub fn get(&self) -> Option<i64> {
        *self.at.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Resolves once the deadline arrives, and clears it so that it fires once.
    ///
    /// A deadline set again while this waits is read again: an earlier one is
    /// honoured, and a cleared one waits for the next.
    pub async fn reached(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let Some(at) = self.get() else {
                changed.await;
                continue;
            };
            let wait = u64::try_from(at.saturating_sub(crate::unix_timestamp())).unwrap_or(0);
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(wait)) => {
                    let mut current = self.at.lock().unwrap_or_else(PoisonError::into_inner);
                    if *current == Some(at) {
                        *current = None;
                        return;
                    }
                }
                () = &mut changed => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn a_deadline_wakes_once_when_it_arrives() {
        let deadline = Arc::new(CoordinationDeadline::default());
        deadline.set(Some(crate::unix_timestamp() + 120));
        tokio::time::timeout(Duration::from_secs(121), deadline.reached())
            .await
            .expect("a deadline two minutes away arrives within them");
        assert_eq!(deadline.get(), None, "a reached deadline is spent");
        assert!(
            tokio::time::timeout(Duration::from_secs(3_600), deadline.reached())
                .await
                .is_err(),
            "and does not fire again until it is set again"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_set_waits_until_something_is() {
        let deadline = Arc::new(CoordinationDeadline::default());
        let waiting = tokio::spawn({
            let deadline = deadline.clone();
            async move { deadline.reached().await }
        });
        tokio::time::sleep(Duration::from_secs(3_600)).await;
        assert!(!waiting.is_finished(), "no cooldown, no wake");
        deadline.set(Some(crate::unix_timestamp() + 5));
        tokio::time::timeout(Duration::from_secs(6), waiting)
            .await
            .expect("a deadline set later is honoured")
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn an_earlier_deadline_set_while_waiting_wins() {
        let deadline = Arc::new(CoordinationDeadline::default());
        deadline.set(Some(crate::unix_timestamp() + 300));
        let waiting = tokio::spawn({
            let deadline = deadline.clone();
            async move { deadline.reached().await }
        });
        tokio::task::yield_now().await;
        deadline.set(Some(crate::unix_timestamp() + 10));
        tokio::time::timeout(Duration::from_secs(11), waiting)
            .await
            .expect("the earlier deadline wakes it")
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_cleared_deadline_does_not_wake() {
        let deadline = Arc::new(CoordinationDeadline::default());
        deadline.set(Some(crate::unix_timestamp() + 10));
        let waiting = tokio::spawn({
            let deadline = deadline.clone();
            async move { deadline.reached().await }
        });
        tokio::task::yield_now().await;
        deadline.set(None);
        tokio::time::sleep(Duration::from_secs(3_600)).await;
        assert!(!waiting.is_finished());
        waiting.abort();
    }

    /// Every pass leaves the supervisor the end of the next cooldown, so a
    /// delivery it held back goes then rather than at the next call or tick.
    #[tokio::test]
    async fn a_pass_leaves_the_supervisor_the_end_of_the_next_cooldown() {
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = swarm_domain::WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let state = crate::AppState::default()
            .with_terminal_host(
                swarm_terminal::HostClient::new("/unreachable/terminal.sock"),
                "fictional-deadline-test-token-0001",
            )
            .with_task_store(store.clone());

        state.deliver_coordination().await;
        assert_eq!(
            state.coordination_deadline().get(),
            None,
            "no cooldown running, nothing to wake for"
        );

        let delivered_at = crate::unix_timestamp();
        store
            .record_coordination_delivery(session, delivered_at)
            .unwrap();
        state.deliver_coordination().await;
        assert_eq!(
            state.coordination_deadline().get(),
            Some(delivered_at + swarm_persistence::COORDINATION_DELIVERY_COOLDOWN_SECONDS)
        );
    }
}
