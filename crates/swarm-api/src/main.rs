use std::{env, net::SocketAddr, path::PathBuf};
use swarm_api::{AppState, router, router_with_web_root};
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
    let address = api_address_from_env()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(%address, "Swarm Next API listening");
    let app = match env::var_os("SWARM_WEB_ROOT") {
        Some(root) => router_with_web_root(state, root),
        None => router(state),
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn api_address_from_env() -> Result<SocketAddr, Box<dyn std::error::Error>> {
    env::var("SWARM_API_BIND").map_or_else(
        |_| Ok(SocketAddr::from(([127, 0, 0, 1], 8765))),
        |value| {
            value.parse::<SocketAddr>().map_err(|error| {
                format!("SWARM_API_BIND must be an IP address and port: {error}").into()
            })
        },
    )
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => {
                tracing::error!(%error, "failed to install termination signal");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    tracing::error!(%error, "failed to install interrupt signal");
                }
            }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install interrupt signal");
    }
}
