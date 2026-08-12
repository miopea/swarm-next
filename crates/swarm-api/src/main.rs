use std::{env, net::SocketAddr, path::PathBuf};
use swarm_api::{AppState, router, router_with_asset_root, router_with_web_root};
use swarm_persistence::TaskStore;
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
    let database_path = database_path_from_env();
    let address = api_address_from_env()?;
    let agent_config_root = agent_config_root_from_env(&database_path);
    let store = TaskStore::open(&database_path)?;
    store.ensure_queen(queen_workspace_from_env().to_string_lossy().as_ref())?;
    let state = env::var("SWARM_OPERATOR_TOKEN")
        .map_or_else(
            |_| AppState::default(),
            |token| AppState::default().with_terminal_host(HostClient::new(terminal_socket), token),
        )
        .with_task_store(store)
        .with_agent_configuration(agent_config_root, mcp_url_from_env(address));
    state.supervise_workers().await;
    let supervisor = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            supervisor.supervise_workers().await;
        }
    });
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(%address, "Swarm Next API listening");
    let app = match (
        env::var_os("SWARM_WEB_ROOT"),
        env::var_os("SWARM_ASSET_ROOT"),
    ) {
        (Some(root), Some(asset_root)) => router_with_asset_root(state, root, asset_root),
        (Some(root), None) => router_with_web_root(state, root),
        (None, _) => router(state),
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn queen_workspace_from_env() -> PathBuf {
    env::var_os("SWARM_QUEEN_WORKSPACE").map_or_else(
        || {
            env::var_os("SWARM_WORKSPACE_ROOTS")
                .map_or_else(|| PathBuf::from("queen"), PathBuf::from)
                .join("queen")
        },
        PathBuf::from,
    )
}

fn database_path_from_env() -> PathBuf {
    env::var_os("SWARM_DATABASE_PATH").map_or_else(
        || {
            env::var_os("HOME").map_or_else(
                || PathBuf::from("swarm-next.sqlite3"),
                |home| {
                    PathBuf::from(home)
                        .join(".local")
                        .join("state")
                        .join("swarm-next")
                        .join("swarm-next.sqlite3")
                },
            )
        },
        PathBuf::from,
    )
}

fn agent_config_root_from_env(database_path: &std::path::Path) -> PathBuf {
    env::var_os("SWARM_AGENT_CONFIG_ROOT").map_or_else(
        || {
            database_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("agents")
        },
        PathBuf::from,
    )
}

fn mcp_url_from_env(address: SocketAddr) -> String {
    env::var("SWARM_MCP_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{}/mcp", address.port()))
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
