use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use swarm_api::bundled_feedback::{FeedbackDestination, feedback_destination};
use swarm_api::{AppState, router, router_with_asset_root, router_with_web_root};
use swarm_persistence::TaskStore;
use swarm_terminal::{HostClient, default_terminal_socket_path};
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Where this Hive files feedback, in order of precedence.
///
/// A bad value DEGRADES rather than refusing to boot, for the reason the public
/// base URL records: nothing here is needed to serve a request, and a Hive that
/// will not start helps nobody.
///
/// THREE CASES, AND THE THIRD IS NEW. An operator who names their own
/// repository and token files there — a Hive whose owner triages their own
/// issues, which is what the environment variables were built for. Half a
/// credential stays a named mistake. What changed is the ordinary case: a fresh
/// install with neither variable set now files into the Swarm repository on the
/// credential this build ships, so reporting a bug about Swarm needs no
/// install-time setup at all and no stranger's report goes out under the
/// operator's own account.
///
/// The shipped credential is NEVER paired with an operator's repository. It is
/// scoped to `issues: write` on one repository, so it is only ever used with
/// the one it names — see `bundled_feedback`. An operator who wants a different
/// destination supplies their own token with it.
///
/// The precedence itself lives in `bundled_feedback::feedback_destination`, a
/// pure function with tests. This reads the environment and applies the result;
/// it does not decide, so the rule and its assertions cannot drift apart.
fn configure_github_feedback(state: AppState) -> AppState {
    match feedback_destination(
        env::var("SWARM_GITHUB_REPOSITORY").ok(),
        env::var("SWARM_GITHUB_TOKEN").ok(),
    ) {
        FeedbackDestination::Operator { repository, token } => {
            match state.clone().with_github_feedback(&repository, &token) {
                Ok(configured) => configured,
                Err(error) => state.with_degraded_subsystem("GitHub feedback", error),
            }
        }
        FeedbackDestination::Bundled { repository, token } => {
            match state.clone().with_github_feedback(repository, token) {
                Ok(configured) => configured,
                // The shipped credential being unusable is a fault in the BUILD,
                // not in anything the operator did, so it is named rather than
                // swallowed — otherwise a release packaged without a working
                // token is indistinguishable from one that simply files locally.
                Err(error) => state.with_degraded_subsystem(
                    "GitHub feedback",
                    format!("the credential this build ships was refused: {error}"),
                ),
            }
        }
        // Half a credential is a mistake worth naming rather than ignoring.
        FeedbackDestination::HalfCredential => state.with_degraded_subsystem(
            "GitHub feedback",
            "set both SWARM_GITHUB_REPOSITORY and SWARM_GITHUB_TOKEN, or neither".to_owned(),
        ),
        // NO CREDENTIAL IS STILL A LEGITIMATE CONFIGURATION, and stays silent
        // here: a build carrying no bundled token — every `cargo build` — keeps
        // reports local. That is now rare rather than ordinary, and the dialog
        // says so at the point of use instead of quietly offering only "Save to
        // this Hive".
        FeedbackDestination::Nowhere => state,
    }
}

/// Opts this Hive in to receiving a repository's open issues as draft tasks.
///
/// A SECOND, EXPLICIT DECISION. Filing feedback is what every Swarm user does;
/// taking delivery of a repository's whole issue list is what its maintainer
/// does. Before this existed both keyed off `SWARM_GITHUB_TOKEN`, so the only
/// thing keeping one operator's backlog off everybody else's task board was
/// that nobody else had set the variable — configuration standing in for
/// design, and about to get worse as more people connect accounts.
///
/// The repository is named here rather than inherited from
/// `SWARM_GITHUB_REPOSITORY` so that a maintainer can triage a repository they do
/// not file feedback into, and so that reading this file tells you which repo
/// arrives on this board.
fn configure_github_issue_intake(state: AppState) -> AppState {
    match (
        env::var("SWARM_GITHUB_ISSUE_INTAKE").ok(),
        env::var("SWARM_GITHUB_TOKEN").ok(),
    ) {
        (Some(repository), Some(token)) => {
            match state.clone().with_github_issue_intake(&repository, &token) {
                Ok(configured) => configured,
                Err(error) => state.with_degraded_subsystem("GitHub issue intake", error),
            }
        }
        // Naming a repository with no credential to read it is a mistake worth
        // reporting, not a silent no-op.
        (Some(_), None) => state.with_degraded_subsystem(
            "GitHub issue intake",
            "SWARM_GITHUB_ISSUE_INTAKE needs SWARM_GITHUB_TOKEN as well".to_owned(),
        ),
        // A token without the opt-in is the ORDINARY case for anyone who is not
        // maintaining the repository, and must stay silent.
        (None, _) => state,
    }
}

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
    // Read once and remembered, because a Hive without it needs to SAY so and
    // the branch that builds the state has already thrown the answer away by
    // the time anything can. Only whether it is present is kept here; the value
    // goes straight into the state and is never logged or reported.
    let operator_token = env::var("SWARM_OPERATOR_TOKEN").ok();
    let mut state = operator_token
        .clone()
        .map_or_else(AppState::default, |token| {
            AppState::default().with_terminal_host(HostClient::new(terminal_socket), token)
        })
        .with_attachment_store(attachment_root_from_database(&database_path))
        .with_database_directory(
            database_path
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        )
        .with_email_attachment_store(email_attachment_root_from_database(&database_path))
        .with_legacy_database_path(legacy_database_path_from_env())
        .with_maintenance_request_path(maintenance_request_path_from_env(&database_path))
        .with_operator_config_path(operator_config_path())
        .with_release_paths(
            release_state_root(&database_path),
            release_apply_request_path(&database_path),
        )
        .with_workspace_roots(workspace_roots)
        .with_task_store(store);
    state = note_missing_operator_token(state, operator_token.as_ref());
    // WEB PUSH IS NOT LOAD-BEARING. A bad VAPID subject used to abort startup,
    // which trades "the operator gets no push notifications" for "the operator
    // has no Hive". See `degrade` for why every one of these is a clone.
    state = degrade(
        state,
        "Web push notifications",
        AppState::with_notifications,
        vapid_subject_from_env(),
    );
    // Setting one of the two development reload paths and not the other used
    // to be fatal. A developer who mistyped an env var got no Hive rather than
    // no reload button, which is the same trade this whole change exists to
    // stop making.
    match development_reload_paths_from_env() {
        Ok(Some((request, status))) => {
            state = state.with_development_reload_paths(request, status);
            if let Some(checkout) = env::var_os("SWARM_DEV_CHECKOUT") {
                state = state.with_development_checkout_path(PathBuf::from(checkout));
            }
        }
        Ok(None) => {}
        Err(error) => state = state.with_degraded_subsystem("Development reload", error),
    }
    if let Ok(manifest_url) = env::var("SWARM_RELEASE_MANIFEST_URL") {
        state = state.with_release_manifest_url(manifest_url);
    }
    if let Ok(public_base_url) = env::var("SWARM_PUBLIC_BASE_URL") {
        // A TYPO IN ONE ENVIRONMENT VARIABLE USED TO BRICK THE HIVE. The value
        // is only ever a place to send someone back to; nothing needs it to
        // serve a request, take a backup, or install a different release.
        state = match state.clone().with_public_base_url(&public_base_url) {
            Ok(configured) => configured,
            Err(error) => state.with_degraded_subsystem("Public base URL", error),
        };
    }
    state = configure_github_feedback(state);
    state = configure_github_issue_intake(state);
    state = state.with_email_oauth_paths(
        email_configuration_path(&database_path),
        email_token_path(&database_path),
    );
    // THE BIND ADDRESS IS SET BEFORE configure_email, AND THE ORDER IS THE FIX.
    //
    // consent_base_url falls back to http://localhost:<port> when no public
    // base URL is configured, and it reads api_bind_address to do it. That
    // fallback was two lines too late to ever fire: configure_email calls
    // with_saved_outlook_oauth, which asks for the consent URL while
    // api_bind_address is still None, and a Hive with a Microsoft registration
    // and no tunnel refuses to start with "Microsoft email OAuth needs an
    // address to come back to".
    //
    // It hid because it needs both conditions AND a restart. A developer
    // registered Outlook, did not restart, and the next restart was an update —
    // so a config fault from days earlier surfaced as a failed upgrade, on a
    // Hive that could then not start on either release.
    state = state.with_api_bind_address(address);
    state = configure_jira(state, &database_path);
    state = configure_email(state, &database_path);
    state = state.with_agent_configuration(agent_config_root, mcp_url_from_env(address));
    state = configure_ops_integrations(state);
    // Recovering queued deliveries is repair, and repair that cannot run is a
    // reason to say so rather than a reason to refuse to start -- a Hive that
    // will not boot delivers nothing at all, which is strictly worse than one
    // that boots with a backlog it could not requeue.
    if let Err(error) = recover_interrupted_deliveries(&state) {
        state = state.with_degraded_subsystem("Queued delivery recovery", error.to_string());
    }
    state.supervise_workers().await;
    start_background_services(&state);
    serve_control_room(state, address).await
}

async fn serve_control_room(
    state: AppState,
    address: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    let (stop_integrity, integrity_stopped) = tokio::sync::oneshot::channel();
    let integrity_monitor = tokio::spawn(swarm_api::monitor_database_integrity(
        state.clone(),
        integrity_stopped,
    ));
    info!(%address, "Swarm API listening");
    let app = match (
        env::var_os("SWARM_WEB_ROOT"),
        env::var_os("SWARM_ASSET_ROOT"),
    ) {
        (Some(root), Some(asset_root)) => router_with_asset_root(state, root, asset_root),
        (Some(root), None) => router_with_web_root(state, root),
        (None, _) => router(state),
    };
    let serving = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = stop_integrity.send(());
        })
        .await;
    if let Err(error) = integrity_monitor.await {
        tracing::warn!(%error, "database integrity monitor could not join during shutdown");
    }
    serving?;
    Ok(())
}

fn configure_ops_integrations(state: AppState) -> AppState {
    match env::var_os("SWARM_OPS_INTEGRATIONS_FILE") {
        Some(path) => state.with_ops_integrations_path(PathBuf::from(path)),
        None => state,
    }
}

/// Says so when there is no operator token, and does not stop the Hive.
///
/// THE LOUDEST SILENT FAILURE THERE IS. Without the token there is no terminal
/// host, so every worker is dead -- and the Hive answered /health with "ok" and
/// an empty degraded list, looking perfectly well. A control room with no
/// running workers is indistinguishable from an idle one, which is worse than
/// email quietly not arriving: nobody is waiting for an email that never comes,
/// but somebody IS waiting for a worker that will never start.
///
/// IT STAYS NON-FATAL. The test is not whether a subsystem is important, it is
/// whether its absence stops the API answering, stops a backup being taken, or
/// stops a different release being installed. A missing token does none of
/// those, and making it fatal would rebuild the trap that cost a developer
/// forty-five minutes.
///
/// The consequence is named, not just the variable: naming
/// `SWARM_OPERATOR_TOKEN` alone does not tell anyone their workers cannot run.
/// Only the token's PRESENCE reaches here; the value never does.
fn note_missing_operator_token(state: AppState, operator_token: Option<&String>) -> AppState {
    if operator_token.is_some() {
        return state;
    }
    state.with_degraded_subsystem(
        "Operator token",
        "SWARM_OPERATOR_TOKEN is not set, so no terminal host is connected and no worker can run. Set it in swarm.env and restart the API.",
    )
}

/// Applies one fallible configuration step, keeping the Hive if it fails.
///
/// THE CLONE IS THE MECHANISM, NOT A COST. Every `with_*` builder takes `self`
/// by value and drops it on the error path, so there is no state left to fall
/// back to once one has failed -- which is precisely why a single bad setting
/// could only ever be expressed as `?` and a dead process. Handing the builder
/// a clone means the pristine state is still here afterwards.
///
/// It also gives the right semantics for free: a subsystem that failed
/// half-way leaves NO partial configuration behind, because the half-built
/// clone is discarded whole. A Hive with broken Jira settings is a Hive with
/// Jira off, never a Hive with Jira on and one field missing.
///
/// `AppState` is `Arc`-backed throughout, so the clone is pointer copies.
fn degrade<T>(
    state: AppState,
    subsystem: &str,
    step: impl FnOnce(AppState, T) -> Result<AppState, String>,
    argument: T,
) -> AppState {
    match step(state.clone(), argument) {
        Ok(configured) => configured,
        Err(error) => {
            tracing::warn!(subsystem, %error, "subsystem disabled; the Hive is still serving");
            state.with_degraded_subsystem(subsystem, error)
        }
    }
}

fn start_background_services(state: &AppState) {
    // Hourly, but a check only happens when the operator asked for daily ones
    // and the last is a day old. Nothing is contacted otherwise.
    let release_poller = std::sync::Arc::new(state.clone());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
        // Startup is a poor moment to make a network call, and a first tick
        // fires immediately.
        interval.tick().await;
        loop {
            interval.tick().await;
            swarm_api::poll_for_release(release_poller.clone()).await;
        }
    });
    let supervisor = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            supervisor.supervise_workers().await;
        }
    });
    // Issues come down as drafts on a slow tick. Slow because nobody files an
    // issue expecting it to appear in a control room within seconds, and a
    // faster poll would spend the API budget for nothing.
    let github_intake = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            github_intake.intake_github_issues().await;
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
    let recovered_task_messages = state.recover_task_messages()?;
    if recovered_task_messages > 0 {
        tracing::warn!(
            recovered_task_messages,
            "interrupted task messages await Queen reconciliation; not replayed"
        );
    }
    if recovered_task_outcomes > 0 {
        tracing::warn!(
            recovered_task_outcomes,
            "crash-interrupted Queen handoffs require operator review"
        );
    }
    let recovered_queen_automation = state.recover_queen_automation()?;
    if recovered_queen_automation > 0 {
        tracing::warn!(
            recovered_queen_automation,
            "crash-interrupted Queen automation requires operator review"
        );
    }
    let recovered_coordinator_actions = state.recover_coordinator_actions()?;
    if recovered_coordinator_actions > 0 {
        tracing::warn!(
            recovered_coordinator_actions,
            "crash-interrupted deterministic coordination requires operator review"
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
                || PathBuf::from("swarm.sqlite3"),
                |home| {
                    PathBuf::from(home)
                        .join(".local")
                        .join("state")
                        .join("swarm")
                        .join("swarm.sqlite3")
                },
            )
        },
        PathBuf::from,
    )
}

/// Where a Legacy Hive keeps its database, newest convention first.
///
/// Legacy moved to `.swarm-legacy` at the rename cutover and this default never
/// followed it, so discovery pointed at a directory that does not exist on any
/// machine that has been through the rename. Reported 2026-08-24: the operator's
/// own Hive, 99 MB of Legacy sitting in `.swarm-legacy` while Swarm looked in
/// `.swarm` and said only that nothing was found.
///
/// Both are tried because an installation that predates the rename still keeps
/// its database at the old path, and only a file that actually exists is
/// chosen — this never silently prefers one live database over another.
const LEGACY_DATABASE_PATHS: &[&str] = &[".swarm-legacy", ".swarm"];

fn legacy_database_path_from_env() -> PathBuf {
    if let Some(configured) = env::var_os("SWARM_LEGACY_DATABASE_PATH") {
        return PathBuf::from(configured);
    }
    let home = env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    legacy_database_path_in(&home)
}

fn legacy_database_path_in(home: &std::path::Path) -> PathBuf {
    let mut first = None;
    for directory in LEGACY_DATABASE_PATHS {
        let candidate = home.join(directory).join("swarm.db");
        if candidate.is_file() {
            return candidate;
        }
        first.get_or_insert(candidate);
    }
    // Nothing found. Report the current convention, so the message a reader
    // gets names the place Legacy would be today rather than a historical one.
    first.unwrap_or_else(|| home.join(".swarm-legacy").join("swarm.db"))
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

/// Configures Jira, or leaves it off and says why.
///
/// The second subsystem taken out of the load-bearing set, chosen because it
/// is the same shape as email and had FIVE distinct ways to refuse to start --
/// more ways than email had, on settings an operator types by hand. An
/// incomplete pair of environment variables here used to be indistinguishable,
/// from the outside, from a corrupt database.
fn configure_jira(state: AppState, database_path: &std::path::Path) -> AppState {
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
        // Nothing in the environment is the ORDINARY case now, not an error:
        // the operator types an Atlassian API token into Settings and this
        // host keeps it. Anything already stored is loaded here, so a restart
        // does not disconnect Jira.
        ((None, None, None), (None, None)) => degrade(
            state,
            "Jira",
            |state, ()| state.with_jira_credentials_path(jira_credentials_path(database_path)),
            (),
        ),
        ((None, None, None), (Some(client_id), Some(client_secret))) => {
            let Some(public_url) = public_url else {
                return state
                    .with_degraded_subsystem("Jira", "Jira OAuth requires SWARM_PUBLIC_BASE_URL");
            };
            degrade(
                state,
                "Jira",
                |state, ()| {
                    state.with_jira_oauth(
                        client_id,
                        client_secret,
                        &public_url,
                        jira_token_path(database_path),
                    )
                },
                (),
            )
        }
        ((Some(base_url), Some(email), Some(api_token)), (None, None)) => degrade(
            state,
            "Jira",
            |state, ()| state.with_jira_configuration(&base_url, email, api_token),
            (),
        ),
        ((Some(_), Some(_), Some(_)), (Some(_), Some(_))) => state.with_degraded_subsystem(
            "Jira",
            "configure either Jira OAuth or Jira API-token authentication, not both",
        ),
        ((None, None, None), _) => {
            state.with_degraded_subsystem("Jira", "Jira OAuth settings are incomplete")
        }
        (_, (None, None)) => {
            state.with_degraded_subsystem("Jira", "Jira API-token settings are incomplete")
        }
        _ => state.with_degraded_subsystem("Jira", "Jira authentication settings are incomplete"),
    }
}

fn jira_credentials_path(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("secrets")
        .join("jira-api-token.json")
}

fn jira_token_path(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("secrets")
        .join("jira-oauth.json")
}

/// Configures Microsoft email, or leaves it off and says why.
///
/// THIS IS THE ONE THAT COST 45 MINUTES. A Hive with a Microsoft registration
/// and no public base URL exited 1 and was restart-looped by systemd, so email
/// being misconfigured meant the control room was gone, no backup could be
/// taken, and no other release could be installed. Email is not needed to
/// serve a request; it never belonged in the set of things that can refuse to
/// start.
fn configure_email(state: AppState, database_path: &std::path::Path) -> AppState {
    let settings = (
        env::var("SWARM_EMAIL_TENANT_ID").ok(),
        env::var("SWARM_EMAIL_OAUTH_CLIENT_ID").ok(),
        env::var("SWARM_EMAIL_OAUTH_CLIENT_SECRET").ok(),
    );
    match settings {
        (None, None, None) => degrade(
            state,
            "Microsoft email",
            |state, ()| state.with_saved_outlook_oauth(),
            (),
        ),
        (Some(tenant_id), Some(client_id), Some(client_secret)) => {
            let Ok(public_url) = env::var("SWARM_PUBLIC_BASE_URL") else {
                return state.with_degraded_subsystem(
                    "Microsoft email",
                    "Microsoft email OAuth requires SWARM_PUBLIC_BASE_URL",
                );
            };
            degrade(
                state,
                "Microsoft email",
                |state, ()| {
                    state.with_outlook_oauth(
                        &tenant_id,
                        client_id,
                        client_secret,
                        &public_url,
                        email_token_path(database_path),
                    )
                },
                (),
            )
        }
        _ => state.with_degraded_subsystem(
            "Microsoft email",
            "Microsoft email OAuth settings are incomplete",
        ),
    }
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

/// Where swarm.env lives. Beside the config the unit already loads, derived
/// the same way the installer places it.
fn operator_config_path() -> PathBuf {
    env::var_os("SWARM_OPERATOR_CONFIG_PATH").map_or_else(
        || {
            // The unit sets this explicitly. This is only for a Hive started
            // by hand, where the conventional location is the best guess
            // available and a wrong guess simply means rotation is refused
            // rather than something being written to the wrong file.
            dirs_config_dir().map_or_else(
                || PathBuf::from("swarm.env"),
                |config| config.join("swarm/swarm.env"),
            )
        },
        PathBuf::from,
    )
}

fn dirs_config_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn release_state_root(database_path: &std::path::Path) -> PathBuf {
    database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn release_apply_request_path(database_path: &std::path::Path) -> PathBuf {
    release_state_root(database_path).join("release-apply.request")
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

    /// Legacy moved to `.swarm-legacy` at the rename cutover; this default did
    /// not follow it. Reported 2026-08-24 with 99 MB of Legacy on the same disk
    /// and Swarm reporting only that nothing was found.
    #[test]
    fn legacy_is_found_at_the_current_location_and_still_at_the_old_one() {
        let home = tempfile::tempdir().unwrap();
        let renamed = home.path().join(".swarm-legacy");
        let original = home.path().join(".swarm");

        // Nothing installed: the path reported is the one Legacy would be at
        // today, so the message names somewhere worth looking.
        assert_eq!(
            legacy_database_path_in(home.path()),
            renamed.join("swarm.db"),
            "with nothing installed it must name the current convention"
        );

        // An installation that predates the rename is still found.
        std::fs::create_dir_all(&original).unwrap();
        std::fs::write(original.join("swarm.db"), b"legacy").unwrap();
        assert_eq!(
            legacy_database_path_in(home.path()),
            original.join("swarm.db"),
            "a Hive that predates the rename keeps its database at the old path"
        );

        // And once renamed, that one wins — it is the live one.
        std::fs::create_dir_all(&renamed).unwrap();
        std::fs::write(renamed.join("swarm.db"), b"legacy").unwrap();
        assert_eq!(
            legacy_database_path_in(home.path()),
            renamed.join("swarm.db"),
            "the current location outranks the historical one"
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

    /// A Hive nobody gave a token to says so, and keeps serving.
    ///
    /// THE ABLATION IS THE WHOLE TEST. Without the row this Hive answers
    /// "ok" with an empty degraded list while no terminal host exists and every
    /// worker is dead — measured, not supposed. A control room with no running
    /// workers looks exactly like an idle one.
    #[test]
    fn a_hive_with_no_operator_token_says_its_workers_cannot_run() {
        let state = super::note_missing_operator_token(AppState::default(), None);
        let degraded = state.degraded_subsystems();
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].subsystem, "Operator token");
        assert!(
            degraded[0].reason.contains("no worker can run"),
            "the consequence has to be named, not just the variable: {}",
            degraded[0].reason
        );
    }

    /// And a Hive that HAS one is not nagged about it.
    ///
    /// The other half: a row that appears on a healthy Hive is noise, and an
    /// operator learns to scroll past exactly the thing that matters.
    #[test]
    fn a_hive_with_a_token_reports_nothing_about_it() {
        let token = String::from("present");
        let state = super::note_missing_operator_token(AppState::default(), Some(&token));
        assert!(state.degraded_subsystems().is_empty());
    }
}
