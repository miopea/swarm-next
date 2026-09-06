//! Process-level shutdown acceptance against an isolated database and shell PTY.
#![cfg(unix)]
use std::{process::Stdio, sync::Arc, time::Duration};
use swarm_domain::{ProviderKind, TaskDispatchState, TaskState};
use swarm_persistence::TaskStore;
use swarm_terminal::{
    HostClient, HostRequest, HostResponse, JournalLimits, ProviderCommand, SessionRegistry,
    TerminalSize,
};

async fn screen(client: &HostClient, session_id: swarm_domain::WorkerSessionId) -> String {
    match client
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
        } => swarm_terminal::snapshot_plain_text(&snapshot.bytes, snapshot.rows, snapshot.columns),
        other => panic!("expected snapshot: {other:?}"),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn sigterm_during_background_paste_finishes_delivery_and_preserves_host() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().canonicalize().unwrap();
    let registry = Arc::new(
        SessionRegistry::new(JournalLimits::new(64 * 1024, 256), 2, [workspace.clone()]).unwrap(),
    );
    let session = registry.spawn(&ProviderCommand {
        executable: "/bin/sh".into(),
        arguments: vec!["-c".into(), "printf '❯ \\nauto mode on\\n'; while IFS= read -r line; do printf 'CONSUMED:%s\\n❯ \\nauto mode on\\n' \"$line\"; done".into()],
        working_directory: workspace.clone(),
    }, TerminalSize::new(24, 240)).unwrap();
    let queen_session = registry
        .spawn(
            &ProviderCommand {
                executable: "/bin/sh".into(),
                arguments: vec![
                    "-c".into(),
                    "printf '❯ \\nauto mode on\\n'; while IFS= read -r line; do :; done".into(),
                ],
                working_directory: workspace.clone(),
            },
            TerminalSize::new(24, 240),
        )
        .unwrap();
    let socket = workspace.join("host.sock");
    let host = swarm_terminal_host::HostServer::bind(&socket, registry).unwrap();
    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(host.run());
    let client = HostClient::new(&socket);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !screen(&client, session.id()).await.contains("auto mode on") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let database = workspace.join("hive.sqlite3");
    let store = TaskStore::open(&database).unwrap();
    let queen = store.ensure_queen(&workspace.to_string_lossy()).unwrap();
    store
        .bind_worker_session(queen.id, queen_session.id())
        .unwrap();
    let worker = store
        .create_worker(
            "SIGTERM fixture",
            ProviderKind::ClaudeCode,
            &workspace.to_string_lossy(),
            false,
            1,
        )
        .unwrap();
    store.bind_worker_session(worker.id, session.id()).unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut api = tokio::process::Command::new(env!("CARGO_BIN_EXE_swarm-api"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("SWARM_DATABASE_PATH", &database)
        .env("SWARM_WORKSPACE_ROOTS", &workspace)
        .env("SWARM_QUEEN_WORKSPACE", &workspace)
        .env("SWARM_TERMINAL_SOCKET", &socket)
        .env("SWARM_AGENT_CONFIG_ROOT", workspace.join("agent-config"))
        .env("SWARM_OPERATOR_CONFIG_PATH", workspace.join("operator.env"))
        .env("SWARM_OPERATOR_TOKEN", "isolated-process-fixture")
        .env("SWARM_API_BIND", address.to_string())
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            assert!(
                api.try_wait().unwrap().is_none(),
                "API exited before readiness"
            );
            if http
                .get(format!("http://{address}/health"))
                .send()
                .await
                .is_ok_and(|reply| reply.status().is_success())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    // Insert through persistence after readiness, so the periodic supervisor,
    // not startup or a still-running HTTP request, owns this delivery.
    let task = store
        .create_task("SIGTERM-PASTE-FIXTURE", &workspace.to_string_lossy())
        .unwrap();
    store.transition_task(task.id, TaskState::Ready).unwrap();
    store
        .assign_task(task.id, session.id())
        .unwrap_or_else(|error| {
            panic!(
                "assignment failed: {error}; profile={:?}; terminal_running={}",
                store.get_worker_profile(worker.id),
                session.reader_running()
            )
        });
    let pasted = tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            let current = screen(&client, session.id()).await;
            if current.contains("SIGTERM-PASTE-FIXTURE") {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!pasted.contains("CONSUMED:"), "signal must precede Enter");
    let process_id = api.id().unwrap();
    assert!(
        std::process::Command::new("/bin/kill")
            .args(["-TERM", &process_id.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let status = tokio::time::timeout(Duration::from_secs(50), api.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success(), "graceful API exit: {status}");
    assert_eq!(
        store.get_task(task.id).unwrap().dispatch_state,
        Some(TaskDispatchState::Delivered)
    );
    let consumed = screen(&client, session.id()).await;
    assert_eq!(
        consumed.matches("CONSUMED:").count(),
        1,
        "exactly one submission: {consumed}"
    );
    assert!(session.reader_running());
    session.stop().unwrap();
    queen_session.stop().unwrap();
    servers.abort_all();
    while servers.join_next().await.is_some() {}
}
