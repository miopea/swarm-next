//! On-demand terminal evidence beside historical coordinator observations.
//! No cache, writes, replay, or task-state inference. At most 32 reads, eight
//! concurrently, each with a two-second observation deadline.
use std::{collections::HashMap, time::Duration};

use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use swarm_persistence::{CoordinatorAttention, TaskStore};
use swarm_terminal::ProviderActivity;

use crate::{
    AppState,
    provider_activity::{ProviderSignals, observe_session},
};

pub(super) async fn observe(
    state: &AppState,
    store: &TaskStore,
    attention: &[CoordinatorAttention],
) -> HashMap<String, Value> {
    let candidates = attention
        .iter()
        .filter(|item| {
            matches!(
                item.kind.as_str(),
                "stale_owned_work_attention"
                    | "assigned_ready_work_not_started_attention"
                    | "owned_work_never_briefed_attention"
            )
        })
        .take(32)
        .cloned()
        .collect::<Vec<_>>();
    stream::iter(candidates)
        .map(|item| async move {
            let profile = store.get_worker_profile(item.worker_id).ok();
            let signals = if let Some(profile) =
                profile.filter(|profile| profile.active_session_id == Some(item.session_id))
            {
                tokio::time::timeout(
                    Duration::from_secs(2),
                    observe_session(state, item.session_id, profile.provider),
                )
                .await
                .ok()
                .flatten()
            } else {
                None
            };
            (
                item.action_id.clone(),
                evidence(signals, crate::unix_timestamp()),
            )
        })
        .buffer_unordered(8)
        .collect()
        .await
}

fn evidence(signals: Option<ProviderSignals>, checked_at: i64) -> Value {
    let activity = match signals.map(|signals| signals.activity) {
        Some(ProviderActivity::Active) => "active",
        Some(ProviderActivity::Resting) => "resting",
        Some(ProviderActivity::AwaitingOperator) => "awaiting_operator",
        Some(ProviderActivity::Unknown) => "unknown",
        None => "unavailable",
    };
    json!({
        "checked_at": checked_at,
        "activity": activity,
        "background_work_visible": signals.map(|signals| signals.background_work),
        "scope": "Current canonical-terminal observation, not a process-tree check or permission to inject. The saved reason and age are historical. Active or awaiting-operator output must not be interrupted; unknown or unavailable is not resting. Recheck task ownership and engagement through guarded delivery before acting."
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    // One real socket exercises successful reads, identity refusal, and cancellation.
    #[allow(clippy::too_many_lines)]
    async fn host_reads_refresh_activity_reject_wrong_sessions_and_cancel_timeout() {
        use swarm_terminal::{HostClient, HostRequest, HostResponse, Resume, TerminalSnapshot};
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("attention.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Fixture",
                swarm_domain::ProviderKind::ClaudeCode,
                "/fixture",
                false,
                1,
            )
            .unwrap();
        let session = "019ff136-7a90-7631-bbc0-f95efd1df576".parse().unwrap();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store.create_task("Unchanged task", "/fixture").unwrap();
        let row = CoordinatorAttention {
            action_id: "saved".into(),
            session_id: session,
            kind: "stale_owned_work_attention".into(),
            worker_id: worker.id,
            worker_name: worker.name,
            task_id: task.id,
            task_title: task.title,
            reason: "Worker was resting".into(),
            observed_at: 1,
            age_seconds: 200,
        };
        let state = AppState::default().with_terminal_host(HostClient::new(&socket), "fixture");
        let host = tokio::spawn(async move {
            for index in 0..5 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let request: HostRequest = serde_json::from_str(&line).unwrap();
                assert!(
                    matches!(request, HostRequest::Read { session_id, .. } if session_id == session)
                );
                if index == 4 {
                    // A stalled host must observe EOF when the bounded read is
                    // cancelled; no background socket/task may remain waiting.
                    let mut byte = [0];
                    assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
                    continue;
                }
                let response = HostResponse::Output {
                    session_id: if index == 2 {
                        "019ff136-7a90-7631-bbc0-f95efd1df577".parse().unwrap()
                    } else {
                        session
                    },
                    running: index != 3,
                    resume: Resume::Snapshot {
                        snapshot: TerminalSnapshot {
                            sequence: index + 1,
                            rows: 24,
                            columns: 100,
                            truncated: false,
                            bytes: if index == 0 {
                                "● Done.\r\n❯ \r\n? for shortcuts"
                            } else {
                                "❯ do the thing\r\n✻ Working… (esc to interrupt)\r\n"
                            }
                            .as_bytes()
                            .to_vec(),
                        },
                    },
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).await.unwrap();
            }
        });
        for expected in [
            "resting",
            "active",
            "unavailable",
            "unavailable",
            "unavailable",
        ] {
            let result = tokio::time::timeout(
                Duration::from_secs(4),
                observe(&state, &store, std::slice::from_ref(&row)),
            )
            .await
            .unwrap();
            assert_eq!(result["saved"]["activity"], expected);
        }
        tokio::time::timeout(Duration::from_secs(1), host)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().state,
            swarm_domain::TaskState::Draft
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .active_session_id,
            Some(session)
        );
    }

    #[tokio::test]
    async fn changed_sessions_are_unavailable_and_observation_is_bounded() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Fixture",
                swarm_domain::ProviderKind::ClaudeCode,
                "/fixture",
                false,
                1,
            )
            .unwrap();
        let task = store.create_task("Fixture task", "/fixture").unwrap();
        let rows = (0..40)
            .map(|index| CoordinatorAttention {
                action_id: format!("attention-{index}"),
                session_id: "019ff136-7a90-7631-bbc0-f95efd1df576".parse().unwrap(),
                kind: "stale_owned_work_attention".into(),
                worker_id: worker.id,
                worker_name: worker.name.clone(),
                task_id: task.id,
                task_title: task.title.clone(),
                reason: "Historical resting observation".into(),
                observed_at: 1,
                age_seconds: 100,
            })
            .collect::<Vec<_>>();
        let result = observe(&AppState::default(), &store, &rows).await;
        assert_eq!(result.len(), 32);
        assert!(
            result
                .values()
                .all(|value| value["activity"] == "unavailable")
        );
        let mut unrelated = rows[0].clone();
        unrelated.kind = "worker_filed_draft_attention".into();
        assert!(
            observe(&AppState::default(), &store, &[unrelated])
                .await
                .is_empty()
        );
    }

    #[test]
    fn current_activity_never_inherits_the_historical_resting_claim() {
        for (activity, expected) in [
            (ProviderActivity::Active, "active"),
            (ProviderActivity::Resting, "resting"),
            (ProviderActivity::AwaitingOperator, "awaiting_operator"),
            (ProviderActivity::Unknown, "unknown"),
        ] {
            let result = evidence(
                Some(ProviderSignals {
                    activity,
                    background_work: true,
                }),
                42,
            );
            assert_eq!(result["activity"], expected);
            assert_eq!(result["checked_at"], 42);
            assert_eq!(result["background_work_visible"], true);
        }
        let absent = evidence(None, 43);
        assert_eq!(absent["activity"], "unavailable");
        assert!(absent["background_work_visible"].is_null());
    }
}
