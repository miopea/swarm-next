use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use axum::http::StatusCode;
use swarm_domain::{ProviderKind, WorkerId, WorkerProfile, WorkerSessionId};
use swarm_terminal::{
    ClaudeConversationStart, CodexConversationStart, HostRequest, HostResponse, TerminalSize,
};

use crate::{
    ApiError, AppState, WorkerViewFacts, task_store, task_store_error, terminal_host::request_host,
    worker_view,
};

pub(super) async fn start_worker_process(
    state: &AppState,
    worker_id: WorkerId,
    size: TerminalSize,
) -> Result<crate::WorkerView, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    let live = reconcile_worker_bindings_unlocked(state).await?;
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    let is_scout = worker_is_scout(state, worker_id)?;
    if let Some(session_id) = profile.active_session_id
        && live.contains_key(&session_id)
    {
        let last_output_at = live.get(&session_id).copied().flatten();
        return Ok(worker_view(
            profile,
            WorkerViewFacts {
                running: true,
                system_role: is_scout.then_some("scout"),
                last_output_at,
                ..WorkerViewFacts::default()
            },
        ));
    }
    let mcp_config = if profile.provider == ProviderKind::ClaudeCode {
        state
            .agent_bridge
            .as_ref()
            .map(|bridge| bridge.ensure_worker_config(worker_id))
            .transpose()
            .map_err(|error| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "agent_config_unavailable",
                    error.to_string(),
                )
            })?
    } else {
        None
    };

    let request = provider_start_request(state, worker_id, &profile, size, mcp_config)?;
    let response = request_host(state, request).await?;
    let HostResponse::SessionStarted { session_id } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    if let Err(error) = task_store(state)?.bind_worker_session(worker_id, session_id) {
        let _ = request_host(state, HostRequest::Stop { session_id }).await;
        return Err(task_store_error(&error));
    }
    state.worker_errors.write().await.remove(&worker_id);
    state.control_room_notify.notify_waiters();
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    let is_scout = worker_is_scout(state, worker_id)?;
    Ok(worker_view(
        profile,
        WorkerViewFacts {
            running: true,
            system_role: is_scout.then_some("scout"),
            ..WorkerViewFacts::default()
        },
    ))
}

/// The path a session starts in, and whether the host must be told to allow a
/// workspace outside the configured roots.
///
/// Shared by the agent start path and the scratch shell so the two cannot drift
/// on the question of what counts as contained.
///
/// BOTH SIDES MUST JUDGE THE SAME PATH.
///
/// This decides whether to send the override; the terminal host then decides
/// whether to honour the roots, and it CANONICALIZES first. Comparing the
/// stored string here while the host compares the resolved target means the
/// two agree on every ordinary path and disagree on exactly one shape: a
/// symlink sitting inside a root that points outside it. There the stored
/// path looks contained, so no override is sent, and the host resolves the
/// target, finds it outside, and refuses.
///
/// That is not hypothetical. The RCG Development Installer's workspace is
/// /home/bschleifer/projects/rcg/rcg-dev-install, a symlink to
/// /home/bschleifer/rcg-dev-install, with the roots set to
/// /home/bschleifer/projects. It had never started a session in its entire
/// existence, and every existence check anyone ran passed, because `is_dir()`
/// follows symlinks.
///
/// Canonicalizing is not a relaxation. The host still applies the same roots
/// rule to the same resolved path it always did; this only stops the caller
/// deciding the question from different evidence. Containment is established
/// at worker creation by `resolve_workspace_path`, which refuses a symlink
/// outright and stores the canonical target — so a worker created through
/// that path can never reach here in this shape, and one that does got its
/// row written some other way.
///
/// A path that cannot be resolved falls back to the stored form rather than
/// failing: the host will refuse it and now says why.
fn workspace_and_root_override(state: &AppState, workspace: &str) -> (PathBuf, bool) {
    let resolved = PathBuf::from(expand_home(workspace));
    let judged = std::fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
    let allow_outside_roots = !state
        .workspace_roots
        .iter()
        .any(|root| judged.starts_with(root));
    (resolved, allow_outside_roots)
}

fn provider_start_request(
    state: &AppState,
    worker_id: WorkerId,
    profile: &WorkerProfile,
    size: TerminalSize,
    mcp_config: Option<PathBuf>,
) -> Result<HostRequest, ApiError> {
    let (worker_workspace, allow_outside_roots) =
        workspace_and_root_override(state, &profile.workspace);
    let request = match profile.provider {
        // This build read a provider it does not recognise, which happens after
        // a rollback to a release predating that provider. The worker stays
        // VISIBLE so one row cannot take down the roster, but it must not start:
        // there is no adapter for it, and falling back to another provider would
        // run the wrong agent against the operator's repository.
        ProviderKind::Unsupported => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "provider_unsupported",
                "this worker uses a provider this version of Swarm does not support; \
                 update Swarm, or change the worker's provider while it is asleep",
            ));
        }
        ProviderKind::ClaudeCode => {
            // A conversation Claude no longer holds is a lost thread, not a
            // reason the worker cannot run. Refusing to start left seventeen
            // workers permanently unstartable with nothing in the product able
            // to clear them, because Claude prunes its own history on a
            // schedule Swarm does not control.
            let resumable = claude_resume_history_available(profile);
            HostRequest::StartClaude {
                workspace: worker_workspace.clone(),
                size,
                conversation: match (
                    profile.provider_conversation_id,
                    profile.has_session_history && resumable,
                ) {
                    (Some(session_id), false) => ClaudeConversationStart::New { session_id },
                    (Some(session_id), true) => ClaudeConversationStart::Resume { session_id },
                    (None, true) => ClaudeConversationStart::Continue,
                    (None, false) => {
                        let session_id = task_store(state)?
                            .assign_provider_conversation(worker_id)
                            .map_err(|error| task_store_error(&error))?;
                        ClaudeConversationStart::New { session_id }
                    }
                },
                mcp_config,
                allow_outside_roots,
            }
        }
        ProviderKind::Codex => HostRequest::StartCodex {
            workspace: worker_workspace,
            size,
            conversation: match (
                profile.provider_conversation_id,
                profile.has_session_history,
            ) {
                (Some(session_id), true) => CodexConversationStart::Resume { session_id },
                (None, true) => CodexConversationStart::Continue,
                _ => CodexConversationStart::New,
            },
            allow_outside_roots,
        },
    };
    Ok(request)
}

/// Whether this worker's saved Claude conversation can be resumed, making it
/// available where the worker's Claude will look for it if it is not there yet.
///
/// Returns false rather than failing when it cannot be found. The worker then
/// starts a fresh conversation under the same session id, which is a lost
/// thread rather than a lost worker — Claude prunes its own history on a
/// schedule Swarm does not control, and refusing to start left workers
/// unstartable with nothing in the product able to clear them.
fn claude_resume_history_available(profile: &WorkerProfile) -> bool {
    let Some(conversation_id) = profile
        .provider_conversation_id
        .filter(|_| profile.has_session_history)
    else {
        return true;
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    let target =
        std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| home.join(".claude"), PathBuf::from);
    let available = ensure_claude_resume_history_between(
        &profile.workspace,
        &conversation_id.to_string(),
        &home.join(".claude/projects"),
        &target.join("projects"),
        &home,
    )
    .is_ok();
    if !available {
        tracing::info!(
            worker = %profile.name,
            "the saved Claude conversation is no longer on this machine; starting a fresh one"
        );
    }
    available
}

/// Resolves a leading `~` against the operator's home.
///
/// A workspace may be stored with one, and only the resume-history lookup
/// expanded it — the start passed the string through verbatim, so the terminal
/// host was asked to open a directory literally named `~` and the worker could
/// never start. It also made the path fail every workspace-root comparison,
/// because no root begins with a tilde.
fn expand_home(workspace: &str) -> String {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return workspace.to_owned();
    };
    expand_home_against(workspace, &home)
}

fn expand_home_against(workspace: &str, home: &Path) -> String {
    if workspace == "~" {
        home.to_string_lossy().into_owned()
    } else if let Some(relative) = workspace.strip_prefix("~/") {
        home.join(relative).to_string_lossy().into_owned()
    } else {
        workspace.to_owned()
    }
}

fn ensure_claude_resume_history_between(
    workspace: &str,
    conversation_id: &str,
    source_root: &Path,
    target_root: &Path,
    home: &Path,
) -> Result<(), std::io::Error> {
    let workspace = expand_home_against(workspace, home);
    let encoded = workspace.replace(['/', '.'], "-");
    let destination_directory = target_root.join(&encoded);
    let destination = destination_directory.join(format!("{conversation_id}.jsonl"));
    if destination.is_file() {
        return Ok(());
    }
    let older_encoded = workspace.replace('/', "-");
    let source = [encoded.as_str(), older_encoded.as_str()]
        .into_iter()
        .map(|directory| {
            source_root
                .join(directory)
                .join(format!("{conversation_id}.jsonl"))
        })
        .find(|path| path.is_file())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "conversation missing"))?;
    std::fs::create_dir_all(&destination_directory)?;
    let temporary = destination.with_extension("jsonl.importing");
    std::fs::copy(source, &temporary)?;
    std::fs::rename(temporary, destination)
}

fn worker_is_scout(state: &AppState, worker_id: WorkerId) -> Result<bool, ApiError> {
    task_store(state)?
        .scout_worker_id()
        .map(|scout_id| scout_id == Some(worker_id))
        .map_err(|error| task_store_error(&error))
}

/// Live worker sessions and, where the terminal host reports it, the wall-clock
/// second each one last produced output.
pub(super) type LiveSessions = HashMap<WorkerSessionId, Option<i64>>;

/// Opens a scratch shell in a worker's workspace WITHOUT binding it to that
/// worker.
///
/// The absence of a binding is the whole feature. Swarm decides whether a worker
/// is working, resting or blocked by reading its terminal, and a shell prompt
/// answers none of those questions — bound as a worker session it would classify
/// as permanently Unknown and, worse, make a SLEEPING worker read as awake.
///
/// Nothing sweeps the session away for being unbound: `reconcile_worker_bindings`
/// runs the other direction, releasing DB bindings whose host sessions have
/// vanished. It never terminates a host session that has no binding.
///
/// The worker is only consulted for its workspace. It is not started, not woken,
/// and its state is not touched.
///
/// # Errors
/// Returns an error when the worker is unknown or the host refuses to spawn.
pub(super) async fn open_worker_shell(
    state: &AppState,
    worker_id: WorkerId,
    size: TerminalSize,
) -> Result<WorkerSessionId, ApiError> {
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    let (workspace, allow_outside_roots) = workspace_and_root_override(state, &profile.workspace);
    // A protocol addition reaches an OLDER host as an unreadable serde error.
    // The terminal host is a separate service that deliberately survives an API
    // reload so worker terminals are not killed, so "the API is new and the host
    // is not" is the ORDINARY state after adding a request, not an edge case.
    // The operator saw: unknown variant `start_shell`, expected one of `ping`,
    // `host_status`, ... which says nothing about what to do.
    let response = request_host(
        state,
        HostRequest::StartShell {
            workspace,
            size,
            allow_outside_roots,
        },
    )
    .await
    .map_err(|error| {
        if error.message.contains("unknown variant") && error.message.contains("start_shell") {
            ApiError::new(
                StatusCode::CONFLICT,
                "terminal_host_too_old",
                "the terminal host is older than this build and does not know how to open a \
                 shell; run the worker engine update to replace it",
            )
        } else {
            error
        }
    })?;
    match response {
        HostResponse::SessionStarted { session_id } => Ok(session_id),
        _ => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        )),
    }
}

pub(super) async fn reconcile_worker_bindings(state: &AppState) -> Result<LiveSessions, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    reconcile_worker_bindings_unlocked(state).await
}

async fn reconcile_worker_bindings_unlocked(state: &AppState) -> Result<LiveSessions, ApiError> {
    let response = request_host(state, HostRequest::ListSessions).await?;
    let HostResponse::Sessions { sessions } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    let live = sessions
        .into_iter()
        .filter(|session| session.running)
        .map(|session| (session.session_id, session.last_output_at))
        .collect::<LiveSessions>();
    let live_ids = live.keys().copied().collect::<HashSet<_>>();
    let released = task_store(state)?
        .release_missing_worker_sessions(&live_ids)
        .map_err(|error| task_store_error(&error))?;
    if released > 0 {
        // Releasing a session detaches a worker from its profile: the roster
        // shows it sleeping while its terminal keeps running under a generated
        // name. That is a large, visible change to make silently, and it went
        // unexplained for hours once because nothing recorded it. An empty
        // report from the host releases every session at once, so the count the
        // host gave is recorded beside the count released.
        tracing::warn!(
            released,
            host_reported_running = live_ids.len(),
            "worker sessions were released because the terminal host no longer reports them"
        );
        state.control_room_notify.notify_waiters();
    }
    Ok(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the start request for a worker whose workspace is `workspace`,
    /// with `root` as the only configured root, and reports whether the
    /// override was sent.
    #[cfg(unix)]
    fn override_sent_for(workspace: &Path, root: &Path) -> bool {
        use swarm_domain::ProviderKind;

        let state = crate::AppState::default().with_workspace_roots(vec![root.to_path_buf()]);
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let profile = store
            .create_worker(
                "Installer",
                ProviderKind::Codex,
                &workspace.to_string_lossy(),
                false,
                1,
            )
            .unwrap();
        let state = state.with_task_store(store);
        match provider_start_request(&state, profile.id, &profile, TerminalSize::default(), None)
            .unwrap()
        {
            HostRequest::StartCodex {
                allow_outside_roots,
                ..
            }
            | HostRequest::StartClaude {
                allow_outside_roots,
                ..
            } => allow_outside_roots,
            other => panic!("unexpected start request: {other:?}"),
        }
    }

    /// A symlink inside a root, pointing outside it, is the one shape where the
    /// two sides of this decision disagreed.
    ///
    /// This side chose whether to send the override; the terminal host decided
    /// whether to honour the roots, and it canonicalizes first. Comparing the
    /// stored string here against the resolved target there agrees on every
    /// ordinary path and splits on exactly this one — the stored path looks
    /// contained, so no override went, and the host resolved the target, found
    /// it outside, and refused.
    ///
    /// The RCG Development Installer sat in exactly this shape and had never
    /// started a session in its entire existence. Every existence check anyone
    /// ran passed, because `is_dir` follows symlinks.
    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_a_root_pointing_outside_it_is_judged_by_where_it_lands() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("projects");
        let outside = directory.path().join("elsewhere");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let link = root.join("dev-install");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        assert!(
            override_sent_for(&link, &root),
            "the target is outside the root, so the host will refuse without the override"
        );
    }

    /// And the ordinary contained case is unchanged — no override is sent, so
    /// the host applies the roots rule exactly as before. Relaxing this is what
    /// would let a symlink dropped in a root run an agent anywhere on the box,
    /// and nothing here does that: the host still applies the same rule to the
    /// same resolved path.
    #[cfg(unix)]
    #[test]
    fn a_real_directory_inside_a_root_still_gets_no_override() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("projects");
        let workspace = root.join("petal");
        std::fs::create_dir_all(&workspace).unwrap();

        assert!(
            !override_sent_for(&workspace, &root),
            "contained work must still be held to the roots"
        );
    }

    /// A symlink inside a root that points somewhere else inside the same root
    /// is contained, and must not be handed an override just for being a link.
    #[cfg(unix)]
    #[test]
    fn a_symlink_that_lands_back_inside_the_root_is_still_contained() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("projects");
        let real = root.join("petal");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.join("petal-alias");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(
            !override_sent_for(&link, &root),
            "resolving inside the root is contained, however it was reached"
        );
    }

    #[test]
    fn first_wake_stages_an_existing_legacy_claude_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let source_root = home.join(".claude/projects");
        let target_root = home.join("isolated/projects");
        let conversation_id = "8e9ed267-7ed8-4b64-94ef-dde3ab17f21a";
        let source = source_root.join("-home-projects-petal");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join(format!("{conversation_id}.jsonl")), b"history").unwrap();

        ensure_claude_resume_history_between(
            "~/projects/petal",
            conversation_id,
            &source_root,
            &target_root,
            Path::new("/home"),
        )
        .unwrap();

        assert_eq!(
            std::fs::read(
                target_root
                    .join("-home-projects-petal")
                    .join(format!("{conversation_id}.jsonl"))
            )
            .unwrap(),
            b"history"
        );
    }

    /// The lookup still reports a missing conversation. What changed is what
    /// the caller does with that: it starts a fresh conversation under the same
    /// session id rather than refusing to start the worker at all.
    ///
    /// Seventeen of the operator's workers were unstartable because Claude had
    /// pruned conversations Swarm still had ids for — a schedule Swarm does not
    /// control — and nothing in the product could clear them. A lost thread is
    /// a smaller loss than a worker that will not run.
    #[test]
    fn a_missing_saved_conversation_is_reported_rather_than_hidden() {
        let directory = tempfile::tempdir().unwrap();
        let error = ensure_claude_resume_history_between(
            "/home/projects/petal",
            "8e9ed267-7ed8-4b64-94ef-dde3ab17f21a",
            &directory.path().join("source"),
            &directory.path().join("target"),
            Path::new("/home"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    /// A workspace stored with a tilde was expanded when looking for resume
    /// history and not when starting the worker, so the terminal host was asked
    /// to open a directory literally named `~`. One worker on the operator's
    /// roster was stored that way and could never start.
    #[test]
    fn a_workspace_written_with_a_tilde_means_the_same_thing_everywhere() {
        let home = Path::new("/home/bschleifer");
        assert_eq!(
            expand_home_against("~/projects/rcg/rcg-dev-install", home),
            "/home/bschleifer/projects/rcg/rcg-dev-install"
        );
        assert_eq!(expand_home_against("~", home), "/home/bschleifer");
        // An absolute path is already what it says, and a tilde inside one is
        // part of the name rather than a reference to home.
        assert_eq!(expand_home_against("/srv/petal", home), "/srv/petal");
        assert_eq!(expand_home_against("/srv/~petal", home), "/srv/~petal");
    }
    /// A worker with nothing saved has nothing to look for, so nothing can
    /// block it. This is the case the caller must not treat as a failure.
    #[test]
    fn a_worker_with_no_saved_conversation_is_never_blocked() {
        let directory = tempfile::tempdir().unwrap();
        let profile = worker_profile(&directory.path().join("projects/petal"), None, false);
        assert!(claude_resume_history_available(&profile));
    }

    /// And a conversation that is present is found where the worker's Claude
    /// will look for it, which is the whole point of the copy.
    #[test]
    fn a_surviving_conversation_is_made_available_to_the_worker() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let workspace = "/home/projects/petal";
        let conversation = "8e9ed267-7ed8-4b64-94ef-dde3ab17f21a";
        let source = home.join("source").join(workspace.replace(['/', '.'], "-"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join(format!("{conversation}.jsonl")), b"history").unwrap();

        ensure_claude_resume_history_between(
            workspace,
            conversation,
            &home.join("source"),
            &home.join("target"),
            Path::new("/home"),
        )
        .unwrap();

        assert!(
            home.join("target")
                .join(workspace.replace(['/', '.'], "-"))
                .join(format!("{conversation}.jsonl"))
                .is_file()
        );
    }

    fn worker_profile(
        workspace: &Path,
        conversation: Option<&str>,
        has_session_history: bool,
    ) -> WorkerProfile {
        WorkerProfile {
            id: swarm_domain::WorkerId::new(),
            hive_id: swarm_domain::HiveId::new(),
            name: "Petal".into(),
            description: String::new(),
            role: swarm_domain::WorkerRole::Worker,
            provider: ProviderKind::ClaudeCode,
            workspace: workspace.to_string_lossy().into_owned(),
            autostart: false,
            position: 0,
            active_session_id: None,
            provider_conversation_id: conversation.map(|id| id.parse().unwrap()),
            has_session_history,
            engagement_expires_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }
}
