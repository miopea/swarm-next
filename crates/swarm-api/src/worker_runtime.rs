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

/// Turns the terminal host's serde error into the sentence that names the fix.
///
/// THE HOST BEING OLDER THAN THE API IS THE ORDINARY STATE, not an edge case.
/// swarm-terminal-host is a separate service that deliberately survives an API
/// reload so worker terminals are not killed mid-turn, so every build that adds
/// a `HostRequest` speaks a protocol the running host does not — until the worker
/// engine update runs.
///
/// Matched on "unknown variant" alone rather than on a named request. The first
/// version of this checked for `start_shell` specifically, which meant the next
/// host request added would reproduce the same unreadable failure and need the
/// same fix again. What the operator saw was serde's variant list: "unknown
/// variant `start_shell`, expected one of `ping`, `host_status`, ..." — accurate
/// and useless, because it says nothing about what to do.
fn host_too_old_for(error: &ApiError, attempted: &str) -> Option<ApiError> {
    error.message.contains("unknown variant").then(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "terminal_host_too_old",
            format!(
                "the terminal host is older than this build and does not know how to \
                 {attempted}; run the worker engine update to replace it"
            ),
        )
    })
}

/// Whether this provider gets a per-worker MCP config minted for it.
///
/// ⚠️ A NAMED FUNCTION BECAUSE THE CONDITION IS THE FIX. It was written inline
/// as `profile.provider == ProviderKind::ClaudeCode`, and that one comparison is
/// the whole of why a Codex worker had no swarm tools: the bridge, the
/// per-worker token and the config file all already existed and were fenced off
/// from it. Reported as "the Codex worker gets the assignment notification but
/// has no swarm tools in its session, so it cannot open the task or read its
/// body, and cannot move it to review."
///
/// The reporter tried to supply a config by hand and could not — "the working
/// config uses a per-worker token that only swarm can mint" — which is exactly
/// right, and is why this had to be fixed here rather than documented.
///
/// Inline, it was also untestable: restoring the old comparison broke no test,
/// which is how a one-line fix ships and then quietly comes back.
///
/// THE ALPHA PROVIDERS STAY OUT on purpose. They start bare, none of their CLIs
/// is installed on this machine, and minting a credential for a surface nobody
/// can run would be issuing a secret on speculation.
const fn provider_reaches_the_board(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::ClaudeCode | ProviderKind::Codex)
}

pub(super) fn automation_admitted(state: &AppState, provider: ProviderKind) -> bool {
    match task_store(state).and_then(|store| {
        store
            .operator_presence(crate::unix_timestamp())
            .map_err(|error| task_store_error(&error))
    }) {
        Ok(presence) => provider.permits_automation_in(presence.mode),
        Err(error) => {
            tracing::warn!(message = %error.message, "automatic startup deferred: presence unavailable");
            false
        }
    }
}

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
    let mcp_config = if provider_reaches_the_board(profile.provider) {
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
    // Refreshed on every start, so a grant approved since the last one takes
    // effect and one whose task has left the board stops existing. The host
    // finds this by deriving it from the MCP config path above; nothing needs
    // to be passed, which is what keeps this off the wire protocol.
    if profile.provider == ProviderKind::ClaudeCode
        && let Some(bridge) = state.agent_bridge.as_ref()
        && let Err(error) = bridge.ensure_worker_settings(worker_id)
    {
        // A grant that cannot be written must not stop a worker starting. The
        // worker then runs without it and is denied exactly as it is today,
        // which is the safe direction and the state it was already in.
        tracing::warn!(%error, "could not write the approved-command grants for this worker");
    }

    let request = provider_start_request(state, worker_id, &profile, size, mcp_config)?;
    let response = request_host(state, request).await.map_err(|error| {
        host_too_old_for(&error, "start this worker's provider").unwrap_or(error)
    })?;
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
            //
            // ⚠️ SWARM NO LONGER DECIDES WHETHER THE CONVERSATION IS STILL THERE,
            // and nothing here reads a directory to find out. It used to look for
            // the transcript under swarm-api's own CLAUDE_CONFIG_DIR while Claude
            // is spawned by the terminal host and inherits THAT service's, so the
            // check could report a conversation gone that Claude could open — and
            // then `New` reused a pinned id Claude still held, Claude refused it,
            // and the worker did not start.
            //
            // A pinned id is now always sent as Resume and the HOST asks Claude,
            // in the environment Claude actually runs in. Correcting which
            // directory this read would have fixed the instance and left a check
            // that can still disagree; the operator chose to remove the class.
            HostRequest::StartClaude {
                workspace: worker_workspace.clone(),
                size,
                conversation: match (
                    profile.provider_conversation_id,
                    profile.has_session_history,
                ) {
                    (Some(session_id), _) => ClaudeConversationStart::Resume { session_id },
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
        // One arm for the three alpha providers, matching the single host
        // request. They start bare: no conversation resume, no MCP config.
        ProviderKind::Gemini | ProviderKind::Grok | ProviderKind::OpenCode => {
            HostRequest::StartAlphaProvider {
                provider: profile.provider,
                workspace: worker_workspace,
                size,
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
            mcp_config,
            allow_outside_roots,
        },
    };
    // ⚠️ SAYS IT COULD NOT CHECK, RATHER THAN ASSUMING THE HOST TOOK IT.
    //
    // `mcp_config` on StartCodex is `#[serde(default)]`, so a host older than
    // this build DROPS it silently and the worker starts with no tools — which
    // is indistinguishable from the defect being fixed. Operator decision
    // 01a05b83 ruled on exactly this shape: "Proceed with a loud warning that
    // says it could not check."
    //
    // The protocol version cannot answer it: this adds a field to an existing
    // request rather than a new request, so PROTOCOL_VERSION does not move and
    // an old host reports the same number a new one does. Nothing here can tell
    // them apart, and that is what is being said out loud.
    if matches!(
        request,
        HostRequest::StartCodex {
            mcp_config: Some(_),
            ..
        }
    ) {
        tracing::info!(
            worker = %profile.name,
            "a Codex worker is starting with swarm tools; a terminal host older than this build \
             ignores that silently and the session comes up without them, exactly as it did before"
        );
    }
    Ok(request)
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

/// Whether the conversation Swarm would resume is still the newest one.
///
/// SWARM PINS A CONVERSATION ID AND NEVER LEARNS WHEN THE REAL ONE MOVES.
/// `assign_provider_conversation` sets it once, and `repoint_provider_conversation`
/// is only ever called by an operator. So when somebody resumes a different
/// conversation inside the session — which is exactly what an operator does to
/// recover a thread — Swarm does not find out, and the next start drops the
/// worker back into the older one. That silently regresses a worker's state.
///
/// Measured across this Hive on 2026-09-02: of 39 Claude workers, 3 were
/// pinned to a conversation that was no longer the newest for their workspace.
/// The worst was 6 hours 34 minutes behind.
///
/// NOT USED TO SWITCH AUTOMATICALLY. The operator declined that: picking the
/// newest on their behalf is a guess about which thread they wanted, and a
/// wrong guess is the same regression from the other direction. This only
/// reports, including reporting that it cannot tell — "we need a way to notify
/// if we don't know" were their words, and an unknown that reads as fine is the
/// failure this whole Hive keeps rediscovering.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum ConversationFreshness {
    /// The pinned conversation is the newest for this workspace.
    Current,
    /// A newer conversation exists, so a start would resume an older thread.
    Stale {
        newest_conversation: String,
        pinned_last_entry: Option<String>,
        newest_last_entry: String,
    },
    /// Swarm cannot establish which is newest. Reported, never assumed fine.
    Unknown { reason: String },
}

/// The last entry timestamp in a transcript, read from its tail.
///
/// Tail-read rather than parsed whole: these run to megabytes and there are
/// dozens. Modification time is NOT usable as the recency signal — Claude
/// rewrites cost-state into old transcripts at startup, so two conversations
/// that last spoke hours apart can share an mtime to the second. Measured on
/// 2026-09-02: Scout's pinned and newest transcripts both showed 11:57 while
/// their last real entries were 00:57 and 02:13.
fn last_entry_timestamp(path: &Path) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    const TAIL: u64 = 256 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = String::new();
    file.take(TAIL).read_to_string(&mut tail).ok()?;
    // The last well-formed timestamp wins. A partial first line from seeking
    // mid-file is simply skipped rather than guessed at.
    tail.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|entry| {
            entry
                .get("timestamp")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .next_back()
}

pub(crate) fn conversation_freshness(
    profile: &WorkerProfile,
    projects_root: &Path,
    home: &Path,
) -> ConversationFreshness {
    if profile.provider != ProviderKind::ClaudeCode {
        return ConversationFreshness::Current;
    }
    let Some(pinned) = profile.provider_conversation_id else {
        // No pin means `--continue`, which takes the newest by definition.
        return ConversationFreshness::Current;
    };
    let workspace = expand_home_against(&profile.workspace, home);
    // ONE OWNER for the encoding, in swarm-persistence. A copy that forgets the
    // '.' returns an empty listing rather than failing, and every caller reads
    // that as "no transcripts".
    let Some(directory) = swarm_persistence::claude_project_directory(projects_root, &workspace)
    else {
        return ConversationFreshness::Unknown {
            reason: "no Claude project directory exists for this workspace".to_owned(),
        };
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return ConversationFreshness::Unknown {
            reason: "the Claude project directory could not be read".to_owned(),
        };
    };
    let mut newest: Option<(String, String)> = None;
    let mut pinned_last: Option<String> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(timestamp) = last_entry_timestamp(&path) else {
            continue;
        };
        if id == pinned.to_string() {
            pinned_last = Some(timestamp.clone());
        }
        if newest
            .as_ref()
            .is_none_or(|(_, best)| timestamp.as_str() > best.as_str())
        {
            newest = Some((id.to_owned(), timestamp));
        }
    }
    let Some((newest_id, newest_timestamp)) = newest else {
        return ConversationFreshness::Unknown {
            reason: "no conversation in this workspace carries a readable entry".to_owned(),
        };
    };
    if newest_id == pinned.to_string() {
        return ConversationFreshness::Current;
    }
    ConversationFreshness::Stale {
        newest_conversation: newest_id,
        pinned_last_entry: pinned_last,
        newest_last_entry: newest_timestamp,
    }
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
    .map_err(|error| host_too_old_for(&error, "open a shell").unwrap_or(error))?;
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

    /// A CODEX WORKER GETS A CONFIG MINTED FOR IT, and an alpha provider does not.
    ///
    /// This is the entire fix for "the Codex worker gets the assignment
    /// notification but has no swarm tools in its session" — one comparison that
    /// used to name Claude alone. It is asserted here because ablating the
    /// original inline condition broke NOTHING: the line that mattered had no
    /// test, which is how a one-line fix ships and then quietly comes back.
    ///
    /// The alpha providers are asserted OUT rather than left unmentioned. They
    /// start bare and none of their CLIs is installed here, so minting a
    /// per-worker bearer token for them would be issuing a credential for a
    /// surface nobody can run.
    #[test]
    fn a_codex_worker_is_given_swarm_tools_and_an_alpha_provider_is_not() {
        assert!(provider_reaches_the_board(ProviderKind::ClaudeCode));
        assert!(
            provider_reaches_the_board(ProviderKind::Codex),
            "this is the defect: the bridge and the token already existed and Codex was fenced out"
        );

        for bare in [
            ProviderKind::Gemini,
            ProviderKind::Grok,
            ProviderKind::OpenCode,
        ] {
            assert!(
                !provider_reaches_the_board(bare),
                "{bare:?} starts bare; minting it a token would issue a credential for a surface \
                 nobody can run"
            );
        }
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

    /// Writes a transcript whose LAST entry carries `last`, and whose mtime is
    /// deliberately touched afterwards so a check keying on modification time
    /// would get the wrong answer.
    fn transcript(directory: &Path, id: &str, last: &str) {
        std::fs::create_dir_all(directory).unwrap();
        let body = format!(
            "{{\"type\":\"user\",\"timestamp\":\"2026-01-01T00:00:00.000Z\"}}\n\
             {{\"type\":\"assistant\",\"timestamp\":\"{last}\"}}\n\
             {{\"type\":\"cost-state\",\"sessionId\":\"{id}\"}}\n"
        );
        std::fs::write(directory.join(format!("{id}.jsonl")), body).unwrap();
    }

    /// THE PINNED CONVERSATION GOING STALE IS THE DEFECT, and it regresses a
    /// worker's state silently: Swarm resumes an older thread and nothing says
    /// so. Measured on the operator's Hive 2026-09-02, 3 of 39 Claude workers
    /// were in this position, the worst 6h34m behind.
    #[test]
    fn a_pinned_conversation_that_is_not_the_newest_is_reported_stale() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("projects/scout");
        let projects = home.path().join(".claude/projects");
        let slug = workspace.to_string_lossy().replace(['/', '.'], "-");
        transcript(
            &projects.join(&slug),
            "11111111-1111-4111-8111-111111111111",
            "2026-09-02T00:57:40.565Z",
        );
        transcript(
            &projects.join(&slug),
            "22222222-2222-4222-8222-222222222222",
            "2026-09-02T02:13:26.742Z",
        );

        let profile = worker_profile(
            &workspace,
            Some("11111111-1111-4111-8111-111111111111"),
            true,
        );
        let freshness = conversation_freshness(&profile, &projects, home.path());

        match freshness {
            ConversationFreshness::Stale {
                newest_conversation,
                newest_last_entry,
                pinned_last_entry,
            } => {
                assert_eq!(newest_conversation, "22222222-2222-4222-8222-222222222222");
                assert_eq!(newest_last_entry, "2026-09-02T02:13:26.742Z");
                assert_eq!(
                    pinned_last_entry.as_deref(),
                    Some("2026-09-02T00:57:40.565Z"),
                    "the pinned thread's own last entry is reported, so the gap is legible"
                );
            }
            other => panic!("expected stale, got {other:?}"),
        }
    }

    /// And the newest being the pinned one is not reported as a problem.
    #[test]
    fn a_pinned_conversation_that_is_the_newest_is_current() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("projects/scout");
        let projects = home.path().join(".claude/projects");
        let slug = workspace.to_string_lossy().replace(['/', '.'], "-");
        transcript(
            &projects.join(&slug),
            "11111111-1111-4111-8111-111111111111",
            "2026-09-02T09:00:00.000Z",
        );
        transcript(
            &projects.join(&slug),
            "22222222-2222-4222-8222-222222222222",
            "2026-09-02T02:13:26.742Z",
        );

        let profile = worker_profile(
            &workspace,
            Some("11111111-1111-4111-8111-111111111111"),
            true,
        );
        assert_eq!(
            conversation_freshness(&profile, &projects, home.path()),
            ConversationFreshness::Current
        );
    }

    /// NOT KNOWING MUST NOT READ AS FINE. The operator's own requirement: "We
    /// need a way to notify if we don't know." A worker whose transcripts are
    /// missing is exactly the case where Swarm would otherwise resume something
    /// and say nothing.
    #[test]
    fn a_workspace_with_no_transcripts_is_reported_unknown_rather_than_current() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("projects/never-ran");
        let projects = home.path().join(".claude/projects");
        std::fs::create_dir_all(&projects).unwrap();

        let profile = worker_profile(
            &workspace,
            Some("11111111-1111-4111-8111-111111111111"),
            true,
        );
        let freshness = conversation_freshness(&profile, &projects, home.path());

        assert!(
            matches!(freshness, ConversationFreshness::Unknown { .. }),
            "an unknown reported as Current is the failure this exists to prevent: {freshness:?}"
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
            ephemeral: false,
            mark: None,
        }
    }
}

/// THE CLASS IS GONE ONLY IF NOTHING HERE LOOKS FOR THE CONVERSATION AGAIN.
///
/// The defect was never the particular directory: it was swarm-api answering a
/// question about a tree the terminal host owns. Correcting which path it read
/// would have fixed the instance and left a check that can still disagree with
/// Claude for some other reason — a permissions case, a path difference, a
/// pruning race. The operator chose to remove the class, so this fails if
/// worker-start ever grows another local oracle for it.
///
/// Deliberately a source scan. A behavioural test would have to construct the
/// disagreement to catch a regression; this catches the reintroduction itself,
/// which is the thing that would be written by somebody who did not read the
/// history.
#[cfg(test)]
mod the_conversation_oracle_is_not_ours {
    const SOURCE: &str = include_str!("worker_runtime.rs");

    /// Split so the scanner does not match its own message, and matching the
    /// ENV READ rather than the name: this file explains in prose why it no
    /// longer reads that variable, and a guard that cannot tell an explanation
    /// from an instruction reports the comment describing the fix as the defect.
    /// It did exactly that on the first run.
    const READS_THE_VARIABLE: &str = concat!("var_os(\"CLAUDE", "_CONFIG_DIR\")");

    #[test]
    fn worker_start_never_decides_for_itself_whether_claude_holds_a_conversation() {
        assert!(
            !SOURCE.contains(READS_THE_VARIABLE),
            "worker start reads Claude's config directory again. Whether Claude \
             holds a conversation is Claude's to answer, in the host, where Claude \
             runs — see conversation_claude_can_open in swarm-terminal-host."
        );
        assert!(
            !SOURCE.contains(concat!("fn claude_resume", "_history_available")),
            "the inference-based availability check is back. It was deleted rather \
             than corrected, on an operator ruling, because a second oracle \
             reintroduces the disagreement being removed."
        );
    }
}
