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
    let workspace_roots = workspace_roots_from_env()?;
    let address = api_address_from_env()?;
    let agent_config_root = agent_config_root_from_env(&database_path);
    let store = TaskStore::open(&database_path)?;
    store.ensure_queen(
        queen_workspace_from_roots(&workspace_roots)
            .to_string_lossy()
            .as_ref(),
    )?;
    let _ = store.promote_project_root_to_scout(
        scout_workspace_from_roots(&workspace_roots)
            .to_string_lossy()
            .as_ref(),
    )?;
    let mut state = env::var("SWARM_OPERATOR_TOKEN")
        .map_or_else(
            |_| AppState::default(),
            |token| AppState::default().with_terminal_host(HostClient::new(terminal_socket), token),
        )
        .with_attachment_store(attachment_root_from_database(&database_path))
        .with_email_attachment_store(email_attachment_root_from_database(&database_path))
        .with_maintenance_request_path(maintenance_request_path_from_env(&database_path))
        .with_workspace_roots(workspace_roots)
        .with_task_store(store)
        .with_notifications(vapid_subject_from_env())?;
    if let Some((request, status)) = development_reload_paths_from_env()? {
        state = state.with_development_reload_paths(request, status);
        if let Some(checkout) = env::var_os("SWARM_DEV_CHECKOUT") {
            state = state.with_development_checkout_path(PathBuf::from(checkout));
        }
    }
    if let Ok(public_base_url) = env::var("SWARM_PUBLIC_BASE_URL") {
        state = state.with_public_base_url(&public_base_url)?;
    }
    state = state.with_email_oauth_paths(
        email_configuration_path(&database_path),
        email_token_path(&database_path),
    );
    state = configure_jira(state, &database_path)?;
    state = configure_email(state, &database_path)?;
    state = state.with_agent_configuration(agent_config_root, mcp_url_from_env(address));
    recover_interrupted_deliveries(&state)?;
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
    let jira_reconciler = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            jira_reconciler.reconcile_jira().await;
        }
    });
    let federation_reconciler = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            federation_reconciler.reconcile_federation().await;
        }
    });
    let email_delivery = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            email_delivery.deliver_email_replies().await;
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

fn recover_interrupted_deliveries(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let recovered = state.recover_decision_deliveries()?;
    if recovered > 0 {
        tracing::warn!(
            recovered,
            "crash-interrupted decision deliveries require operator review"
        );
    }
    let recovered_task_dispatches = state.recover_task_dispatches()?;
    if recovered_task_dispatches > 0 {
        tracing::warn!(
            recovered_task_dispatches,
            "crash-interrupted task briefings require operator review"
        );
    }
    let recovered_notifications = state.recover_notification_deliveries()?;
    if recovered_notifications > 0 {
        tracing::warn!(
            recovered_notifications,
            "crash-interrupted tagged notifications were safely requeued"
        );
    }
    let recovered_task_outcomes = state.recover_task_outcomes()?;
    if recovered_task_outcomes > 0 {
        tracing::warn!(
            recovered_task_outcomes,
            "crash-interrupted Queen handoffs require operator review"
        );
    }
    let recovered_jira_transitions = state.recover_jira_transition_deliveries()?;
    if recovered_jira_transitions > 0 {
        tracing::warn!(
            recovered_jira_transitions,
            "crash-interrupted Jira updates require reconciliation"
        );
    }
    let recovered_jira_comments = state.recover_jira_comment_deliveries()?;
    if recovered_jira_comments > 0 {
        tracing::warn!(
            recovered_jira_comments,
            "crash-interrupted Jira comments require reconciliation"
        );
    }
    let recovered_email_replies = state.recover_email_reply_deliveries()?;
    if recovered_email_replies > 0 {
        tracing::warn!(
            recovered_email_replies,
            "crash-interrupted email replies require operator review"
        );
    }
    Ok(())
}

fn vapid_subject_from_env() -> String {
    env::var("SWARM_VAPID_SUBJECT")
        .unwrap_or_else(|_| "mailto:operator@swarm-next.local".to_owned())
}

fn workspace_roots_from_env() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let roots = env::var_os("SWARM_WORKSPACE_ROOTS").map_or_else(
        || env::current_dir().map(|directory| vec![directory]),
        |value| Ok(env::split_paths(&value).collect::<Vec<_>>()),
    )?;
    if roots.is_empty() || roots.iter().any(|root| !root.is_absolute()) {
        return Err("SWARM_WORKSPACE_ROOTS must contain at least one absolute path".into());
    }
    Ok(roots)
}

fn queen_workspace_from_roots(roots: &[PathBuf]) -> PathBuf {
    env::var_os("SWARM_QUEEN_WORKSPACE").map_or_else(
        || {
            roots
                .first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("queen")
        },
        PathBuf::from,
    )
}

fn scout_workspace_from_roots(roots: &[PathBuf]) -> PathBuf {
    env::var_os("SWARM_SCOUT_WORKSPACE").map_or_else(
        || {
            let first = roots.first().cloned().unwrap_or_else(|| PathBuf::from("."));
            if roots.len() == 1 {
                return first;
            }
            first
                .ancestors()
                .find(|candidate| roots.iter().all(|root| root.starts_with(candidate)))
                .map_or_else(|| first.clone(), PathBuf::from)
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

fn attachment_root_from_database(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("attachments")
}

fn email_attachment_root_from_database(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("email-attachments")
}

fn configure_jira(
    mut state: AppState,
    database_path: &std::path::Path,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let api_token = (
        env::var("SWARM_JIRA_BASE_URL").ok(),
        env::var("SWARM_JIRA_EMAIL").ok(),
        env::var("SWARM_JIRA_API_TOKEN").ok(),
    );
    let oauth = (
        env::var("SWARM_JIRA_OAUTH_CLIENT_ID").ok(),
        env::var("SWARM_JIRA_OAUTH_CLIENT_SECRET").ok(),
    );
    let public_url = env::var("SWARM_PUBLIC_BASE_URL").ok();
    match (api_token, oauth) {
        ((None, None, None), (None, None)) => {}
        ((None, None, None), (Some(client_id), Some(client_secret))) => {
            let public_url = public_url.ok_or("Jira OAuth requires SWARM_PUBLIC_BASE_URL")?;
            state = state.with_jira_oauth(
                client_id,
                client_secret,
                &public_url,
                jira_token_path(database_path),
            )?;
        }
        ((Some(base_url), Some(email), Some(api_token)), (None, None)) => {
            state = state.with_jira_configuration(&base_url, email, api_token)?;
        }
        ((Some(_), Some(_), Some(_)), (Some(_), Some(_))) => {
            return Err(
                "configure either Jira OAuth or Jira API-token authentication, not both".into(),
            );
        }
        ((None, None, None), _) => return Err("Jira OAuth settings are incomplete".into()),
        (_, (None, None)) => return Err("Jira API-token settings are incomplete".into()),
        _ => return Err("Jira authentication settings are incomplete".into()),
    }
    Ok(state)
}

fn jira_token_path(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("secrets")
        .join("jira-oauth.json")
}

fn configure_email(
    mut state: AppState,
    database_path: &std::path::Path,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let settings = (
        env::var("SWARM_EMAIL_TENANT_ID").ok(),
        env::var("SWARM_EMAIL_OAUTH_CLIENT_ID").ok(),
        env::var("SWARM_EMAIL_OAUTH_CLIENT_SECRET").ok(),
    );
    match settings {
        (None, None, None) => state = state.with_saved_outlook_oauth()?,
        (Some(tenant_id), Some(client_id), Some(client_secret)) => {
            let public_url = env::var("SWARM_PUBLIC_BASE_URL")
                .map_err(|_| "Microsoft email OAuth requires SWARM_PUBLIC_BASE_URL")?;
            state = state.with_outlook_oauth(
                &tenant_id,
                client_id,
                client_secret,
                &public_url,
                email_token_path(database_path),
            )?;
        }
        _ => return Err("Microsoft email OAuth settings are incomplete".into()),
    }
    Ok(state)
}

fn email_token_path(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("secrets")
        .join("email-oauth.json")
}

fn email_configuration_path(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("secrets")
        .join("email-oauth-config.json")
}

fn maintenance_request_path_from_env(database_path: &std::path::Path) -> PathBuf {
    env::var_os("SWARM_MAINTENANCE_REQUEST_PATH").map_or_else(
        || {
            database_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("worker-engine-maintenance.request")
        },
        PathBuf::from,
    )
}

fn development_reload_paths_from_env() -> Result<Option<(PathBuf, PathBuf)>, &'static str> {
    match (
        env::var_os("SWARM_DEV_RELOAD_REQUEST_PATH"),
        env::var_os("SWARM_DEV_RELOAD_STATUS_PATH"),
    ) {
        (None, None) => Ok(None),
        (Some(request), Some(status)) => Ok(Some((PathBuf::from(request), PathBuf::from(status)))),
        _ => Err("development reload request and status paths must be configured together"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scout_owns_the_single_configured_projects_root() {
        assert_eq!(
            scout_workspace_from_roots(&[PathBuf::from("/home/operator/projects")]),
            PathBuf::from("/home/operator/projects")
        );
    }

    #[test]
    fn scout_uses_the_common_parent_for_multiple_repository_roots() {
        assert_eq!(
            scout_workspace_from_roots(&[
                PathBuf::from("/home/operator/projects/personal"),
                PathBuf::from("/home/operator/projects/work"),
            ]),
            PathBuf::from("/home/operator/projects")
        );
    }
}
