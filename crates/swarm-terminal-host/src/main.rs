use std::{env, path::PathBuf, sync::Arc};

use swarm_terminal::{
    HistoryLimits, HistoryStore, JournalLimits, SessionRegistry, default_terminal_history_path,
    default_terminal_socket_path,
};
use swarm_terminal_host::HostServer;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod mcp_proxy;
mod provider_session_start;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() == Some("provider-session-start") {
        return provider_session_start::run(false).await.map_err(Into::into);
    }
    if env::args().nth(1).as_deref() == Some("provider-resume-end") {
        return provider_session_start::run(true).await.map_err(Into::into);
    }
    if env::args().nth(1).as_deref() == Some("mcp-proxy") {
        return mcp_proxy::run().await.map_err(Into::into);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "swarm_terminal_host=info".into()),
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
    let history = Arc::new(HistoryStore::open(history_path, history_limits)?);
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
    tokio::select! {
        result = server.run() => result?,
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
