use std::{env, net::SocketAddr, path::PathBuf};
use swarm_api::{AppState, router};
use swarm_terminal::{HostClient, default_terminal_socket_path};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "swarm_api=info".into()),
        )
        .init();
    let terminal_socket = env::var_os("SWARM_TERMINAL_SOCKET")
        .map_or_else(default_terminal_socket_path, PathBuf::from);
    let state = env::var("SWARM_OPERATOR_TOKEN").map_or_else(
        |_| AppState::default(),
        |token| AppState::default().with_terminal_host(HostClient::new(terminal_socket), token),
    );
    let address = SocketAddr::from(([127, 0, 0, 1], 8765));
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(%address, "Swarm Next API listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal");
    }
}
