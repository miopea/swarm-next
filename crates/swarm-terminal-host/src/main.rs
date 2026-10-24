use std::{env, path::PathBuf, sync::Arc};

use swarm_terminal::{
    HistoryLimits, HistoryStore, JournalLimits, SessionRegistry, default_terminal_history_path,
    default_terminal_socket_path,
};
use swarm_terminal_host::{
    HostServer,
    history_retention::{RETENTION_CHECK_SECONDS, apply_requested_history_retention},
};
use tracing::info;
use tracing_subscriber::EnvFilter;

mod mcp_proxy;
mod provider_session_start;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() == Some("provider-interview") {
        return provider_session_start::run_interview()
            .await
            .map_err(Into::into);
    }
    if env::args().nth(1).as_deref() == Some("provider-turn") {
        return provider_session_start::run_turn().await.map_err(Into::into);
    }
    if env::args().nth(1).as_deref() == Some("provider-session-start") {
        return provider_session_start::run(false).await.map_err(Into::into);
    }
    if env::args().nth(1).as_deref() == Some("provider-resume-end") {
        return provider_session_start::run(true).await.map_err(Into::into);
    }
    if env::args().nth(1).as_deref() == Some("mcp-proxy") {
        return mcp_proxy::run().await.map_err(Into::into);
    }
    // ⚠️ THE LIBRARY IS WHERE THE DECISIONS ARE MADE, so it has to be in the
    // filter. The default named only `swarm_terminal_host`, the BINARY crate,
    // while every session, capture and provider-gate decision happens in
    // `swarm_terminal`, the library. So a warning added there ran correctly and
    // was silently discarded before reaching the journal.
    //
    // Found 2026-09-12 after adding twelve refusal warnings to explain why
    // native interview capture was failing, restarting the host to pick them
    // up, and reading a journal with 43 WARN lines and none of mine. The
    // instrumentation was never the problem; the filter was, and it made a
    // working diagnostic indistinguishable from a branch that never fired.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "swarm_terminal_host=info,swarm_terminal=info".into()),
        )
        .init();

    let current_directory = env::current_dir()?;
    let allowed_roots = env::var_os("SWARM_WORKSPACE_ROOTS").map_or_else(
        || vec![current_directory],
        |value| env::split_paths(&value).collect::<Vec<_>>(),
    );
    let socket_path = env::var_os("SWARM_TERMINAL_SOCKET")
        .map_or_else(default_terminal_socket_path, PathBuf::from);
    let history_path = env::var_os("SWARM_TERMINAL_HISTORY_DIR")
        .map_or_else(default_terminal_history_path, PathBuf::from);
    let history_limits = history_limits_from_env()?;
    let history = Arc::new(HistoryStore::open(&history_path, history_limits)?);
    // The Hive's retention, applied before anything is served (ADR 0118).
    apply_requested_history_retention(&history, &history_path);
    let retention_history = Arc::clone(&history);
    let history_diagnostics = history.diagnostics()?;
    info!(
        retained_bytes = history_diagnostics.retained_bytes,
        max_total_bytes = history_diagnostics.limits.max_total_bytes,
        max_session_bytes = history_diagnostics.limits.max_session_bytes,
        max_age_seconds = history_diagnostics.limits.max_age_seconds,
        recovered_truncated_bytes = history_diagnostics.recovered_truncated_bytes,
        recovered_corrupt_segments = history_diagnostics.recovered_corrupt_segments,
        "terminal history ready"
    );
    let registry = Arc::new(SessionRegistry::new_with_history(
        JournalLimits::default(),
        32,
        allowed_roots,
        Some(history),
    )?);
    let server = HostServer::bind(socket_path, registry)?;
    // ⚠️ OWNED BY THIS SELECT, so it ends with the host. One bounded check a
    // minute: the retention the Hive asks for, applied while the host runs,
    // because a remote restart is exactly what ADR 0113 rules out.
    let retention_watch = async move {
        let mut tick =
            tokio::time::interval(std::time::Duration::from_secs(RETENTION_CHECK_SECONDS));
        loop {
            tick.tick().await;
            let history = Arc::clone(&retention_history);
            let dir = history_path.clone();
            let _ = tokio::task::spawn_blocking(move || {
                apply_requested_history_retention(&history, &dir)
            })
            .await;
        }
    };
    tokio::select! {
        result = server.run() => result?,
        () = retention_watch => {}
        result = shutdown_signal() => {
            result?;
            info!("terminal host received graceful shutdown signal");
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

fn history_limits_from_env() -> Result<HistoryLimits, Box<dyn std::error::Error>> {
    let defaults = HistoryLimits::default();
    Ok(HistoryLimits::new(
        env_u64(
            "SWARM_TERMINAL_HISTORY_MAX_RECORD_BYTES",
            defaults.max_record_bytes,
        )?,
        env_u64(
            "SWARM_TERMINAL_HISTORY_MAX_SEGMENT_BYTES",
            defaults.max_segment_bytes,
        )?,
        env_u64(
            "SWARM_TERMINAL_HISTORY_MAX_SESSION_BYTES",
            defaults.max_session_bytes,
        )?,
        env_u64(
            "SWARM_TERMINAL_HISTORY_MAX_TOTAL_BYTES",
            defaults.max_total_bytes,
        )?,
        env_u64(
            "SWARM_TERMINAL_HISTORY_MAX_AGE_SECONDS",
            defaults.max_age_seconds,
        )?,
    ))
}

fn env_u64(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|error| format!("{name} must be an unsigned integer: {error}").into())
    })
}
