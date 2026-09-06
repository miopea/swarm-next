use std::{net::SocketAddr, num::NonZeroU32, path::Path};

use swarm_application::SupportService;
use swarm_persistence::SupportStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Explicit separate database; never derive this from SWARM_DATABASE_PATH.
    let database = std::env::var("SWARM_SUPPORT_DATABASE")?;
    let capacity: NonZeroU32 = std::env::var("SWARM_SUPPORT_CAPACITY")?.parse()?;
    let address: SocketAddr = std::env::var("SWARM_SUPPORT_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:4281".into())
        .parse()?;
    let service = SupportService::new(SupportStore::open(Path::new(&database), capacity)?);
    let admin = match std::env::var("SWARM_SUPPORT_ADMIN_TOKEN") {
        Ok(token) => Some(swarm_support::AdminCredential::new(&token)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("support Admin credential must be valid Unicode".into());
        }
    };
    let ops = match std::env::var("SWARM_SUPPORT_OPS_TOKEN") {
        Ok(token) => Some(swarm_support::OpsConfiguration::new(
            &token,
            &std::env::var("SWARM_SUPPORT_ENVIRONMENT")?,
            std::env::var("SWARM_SUPPORT_BUILD_SHA").ok(),
            Path::new(&database).canonicalize()?,
            std::env::var("SWARM_SUPPORT_MIN_FREE_MIB")?.parse()?,
        )?),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("support Ops credential must be valid Unicode".into());
        }
    };
    let router = match (admin, ops) {
        (admin, Some(ops)) => swarm_support::router_with_ops(service, admin, ops)?,
        (Some(admin), None) => swarm_support::router_with_admin(service, admin),
        (None, None) => swarm_support::router(service),
    };
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let shutdown = async move {
        #[cfg(unix)]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    };
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}
