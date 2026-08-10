use std::{env, path::PathBuf, sync::Arc};

use swarm_terminal::{JournalLimits, SessionRegistry, default_terminal_socket_path};
use swarm_terminal_host::HostServer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let registry = Arc::new(SessionRegistry::new(
        JournalLimits::default(),
        32,
        allowed_roots,
    )?);
    HostServer::bind(socket_path, registry)?.run().await?;
    Ok(())
}
