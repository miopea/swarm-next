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
    start_worker_process_unlocked(state, worker_id, size).await
}

/// None means cancelled, policy-deferred, or draining, not a failed attempt.
pub(super) async fn revive_worker_process(
    state: &AppState,
    worker_id: WorkerId,
    size: TerminalSize,
) -> Result<Option<crate::WorkerView>, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    let store = task_store(state)?;
    if !store
        .worker_revival_pending(worker_id)
        .map_err(|error| task_store_error(&error))?
    {
        return Ok(None);
    }
    let profile = store
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    if !automation_admitted(state, profile.provider) {
        return Ok(None);
    }
    // Recheck after acquiring ownership: another maintenance run may have
    // started after the supervisor's earlier host observation.
    let host = crate::maintenance::host_status_snapshot(state).await?;
    if host.draining {
        return Ok(None);
    }
    let result = start_worker_process_unlocked(state, worker_id, size).await;
    if let Err(error) = &result {
        state
            .worker_errors
            .write()
            .await
            .insert(worker_id, error.message.clone());
    }
    // Start outcome and promise settlement share the same lifecycle lock.
    // A caller must not clear a newer promise or overwrite a newer success
    // after this operation gives up ownership.
    store
        .clear_worker_revival_intent(worker_id)
        .map_err(|error| task_store_error(&error))?;
    result.map(Some)
}

async fn start_worker_process_unlocked(
    state: &AppState,
    worker_id: WorkerId,
    size: TerminalSize,
) -> Result<crate::WorkerView, ApiError> {
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
                (Some(session_id), _) => CodexConversationStart::Resume { session_id },
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
    /// No unresolved drift: the default is provider-confirmed, or is the newest
    /// transcript available to the legacy diagnostic.
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

/// One work allowance shared by the complete diagnostic scan, not per worker.
/// The deadline is cooperative: it cannot interrupt a stalled filesystem call.
pub(crate) struct ConversationScanBudget {
    entries: usize,
    bytes: u64,
    deadline: std::time::Instant,
    pub(crate) exhausted: bool,
}

impl ConversationScanBudget {
    pub(crate) fn new() -> Self {
        Self {
            entries: 4096,
            bytes: 64 * 1024 * 1024,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(6),
            exhausted: false,
        }
    }

    fn reserve(&mut self, entries: usize, bytes: u64) -> bool {
        if self.exhausted
            || std::time::Instant::now() >= self.deadline
            || entries > self.entries
            || bytes > self.bytes
        {
            self.exhausted = true;
            return false;
        }
        self.entries -= entries;
        self.bytes -= bytes;
        true
    }
}

/// Reads entry time from a bounded tail, not mtime: provider cost-state updates
/// can touch an old transcript without advancing its conversation.
fn last_entry_timestamp(path: &Path, budget: &mut ConversationScanBudget) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    use std::os::unix::fs::OpenOptionsExt as _;
    const TAIL: u64 = 256 * 1024;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    let length = metadata.len();
    let read_limit = length.min(TAIL);
    if !budget.reserve(0, read_limit) {
        return None;
    }
    let start = length.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = String::new();
    file.take(read_limit).read_to_string(&mut tail).ok()?;
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
    budget: &mut ConversationScanBudget,
    confirmed: &HashMap<WorkerSessionId, swarm_domain::ProviderConversationSelection>,
) -> ConversationFreshness {
    if profile.provider == ProviderKind::ClaudeCode
        && profile
            .active_session_id
            .and_then(|session| confirmed.get(&session))
            .is_some_and(|selection| {
                Some(selection.conversation) == profile.provider_conversation_id
            })
    {
        // The engine and persistence already agree on the current session's
        // selected conversation. Another file's timestamp cannot overrule it.
        return ConversationFreshness::Current;
    }
    if !budget.reserve(1, 0) {
        return ConversationFreshness::Unknown {
            reason: "conversation scan limit reached".into(),
        };
    }
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
    for entry in entries {
        if !budget.reserve(1, 0) {
            break;
        }
        let Ok(entry) = entry else { continue };
        // A transcript is a regular file. Never open a pipe/device or follow a
        // symlink while performing a best-effort diagnostic scan.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(timestamp) = last_entry_timestamp(&path, budget) else {
            if budget.exhausted {
                break;
            }
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
    if !budget.reserve(0, 0) {
        return ConversationFreshness::Unknown {
            reason: "conversation scan limit reached".into(),
        };
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

fn reconcile_worker_context(
    state: &AppState,
    session: &swarm_terminal::HostSessionSummary,
) -> Result<(), ApiError> {
    if let (Some(attempt), Some(observation)) = (session.recovery_attempt, session.provider_start)
        && task_store(state)?
            .reconcile_provider_start(
                session.session_id,
                attempt,
                observation.kind,
                observation.conversation,
            )
            .map_err(|error| task_store_error(&error))?
            .is_some()
    {
        state.control_room_notify.notify_waiters();
    }
    if let Some(selection) = session.provider_selection
        && task_store(state)?
            .reconcile_provider_selection(session.session_id, selection)
            .map_err(|error| task_store_error(&error))?
    {
        state.control_room_notify.notify_waiters();
    }
    Ok(())
}

async fn stop_context_request(
    state: &AppState,
    request: HostRequest,
) -> Result<HostResponse, ApiError> {
    tokio::time::timeout(std::time::Duration::from_secs(3), request_host(state, request))
        .await
        .map_err(|_| ApiError::new(StatusCode::GATEWAY_TIMEOUT, "worker_stop_unconfirmed", "Worker stop or context preservation could not be confirmed; retained evidence is not discarded."))?
}

/// Stops a worker without discarding its final engine-owned context evidence.
/// Callers keep their lifecycle/maintenance ownership until releasing the binding.
pub(super) async fn stop_worker_session_preserving_context(
    state: &AppState,
    session_id: WorkerSessionId,
) -> Result<(), ApiError> {
    let HostResponse::Pong { protocol_version } =
        stop_context_request(state, HostRequest::Ping).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "Worker engine did not identify its stop protocol.",
        ));
    };
    if !(swarm_terminal::TERMINAL_CONTROL_PROTOCOL_VERSION..=swarm_terminal::PROTOCOL_VERSION)
        .contains(&protocol_version)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "worker_engine_incompatible",
            "Worker engine version is not supported for context-preserving stop.",
        ));
    }
    let retained = protocol_version >= 15;
    if retained
        && !matches!(
            stop_context_request(state, HostRequest::StopRetained { session_id }).await?,
            HostResponse::Acknowledged
        )
    {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "worker_stop_unconfirmed",
            "Worker engine did not confirm retained stop.",
        ));
    }
    let HostResponse::Sessions { sessions } =
        stop_context_request(state, HostRequest::ListSessions).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "Worker engine did not return final context evidence.",
        ));
    };
    let session = sessions
        .iter()
        .find(|session| session.session_id == session_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "worker_context_unavailable",
                "Worker context was not available; no saved-context claim was made.",
            )
        })?;
    if retained && !session.stop_pending_release {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "worker_stop_unconfirmed",
            "Worker engine has not frozen final context evidence.",
        ));
    }
    reconcile_worker_context(state, session)?;
    if !retained {
        tracing::warn!(%session_id, protocol_version, "legacy engine stop can preserve only the pre-stop conversation snapshot");
    }
    match stop_context_request(state, HostRequest::Stop { session_id }).await? {
        HostResponse::Acknowledged => Ok(()),
        _ => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "worker_stop_unconfirmed",
            "Worker engine did not acknowledge stop cleanup.",
        )),
    }
}

fn reconcile_recovery_successors(
    state: &AppState,
    sessions: &[swarm_terminal::HostSessionSummary],
) -> Result<(), ApiError> {
    let by_id = sessions
        .iter()
        .map(|session| (session.session_id, session))
        .collect::<HashMap<_, _>>();
    for previous in sessions {
        let Some(swarm_terminal::ContinuationRecoveryOutcome::SessionCreated { session_id }) =
            previous.continuation_recovery
        else {
            continue;
        };
        let Some(successor) = by_id.get(&session_id) else {
            continue;
        };
        let (Some(previous_attempt), Some(successor_attempt)) =
            (previous.recovery_attempt, successor.recovery_attempt)
        else {
            continue;
        };
        if previous.running || previous.stop_pending_release {
            continue;
        }
        if task_store(state)?
            .reconcile_continuation_successor(
                previous.session_id,
                previous_attempt,
                successor.session_id,
                successor_attempt,
            )
            .map_err(|error| task_store_error(&error))?
        {
            state.control_room_notify.notify_waiters();
        }
    }
    Ok(())
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
    // Session order is arbitrary. Transfer the receipt before consuming the
    // successor's startup evidence or retiring the original dead binding.
    reconcile_recovery_successors(state, &sessions)?;
    for session in &sessions {
        // The engine authenticated this evidence while the child was alive.
        // It can exit before our next read; retain its final conversation before
        // releasing the binding. Persistence still rejects replaced/manual pins.
        reconcile_worker_context(state, session)?;
        if session.stop_pending_release
            && !matches!(
                stop_context_request(
                    state,
                    HostRequest::Stop {
                        session_id: session.session_id,
                    },
                )
                .await?,
                HostResponse::Acknowledged
            )
        {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "worker_stop_unconfirmed",
                "Worker engine did not acknowledge retained-session cleanup.",
            ));
        }
    }
    cleanup_recovery_parents(state, &sessions).await?;
    let live = sessions
        .into_iter()
        .filter(|session| session.running && !session.stop_pending_release)
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

async fn cleanup_recovery_parents(
    state: &AppState,
    sessions: &[swarm_terminal::HostSessionSummary],
) -> Result<(), ApiError> {
    for previous in sessions {
        let Some(swarm_terminal::ContinuationRecoveryOutcome::SessionCreated { session_id }) =
            previous.continuation_recovery
        else {
            continue;
        };
        if previous.running || previous.stop_pending_release || previous.session_id == session_id {
            continue;
        }
        if !task_store(state)?
            .continuation_successor_bound(previous.session_id, session_id)
            .map_err(|error| task_store_error(&error))?
        {
            continue;
        }
        // Only the ended parent's immutable identity is released. The durable
        // successor receipt survives an API failure or a lost cleanup reply.
        if !matches!(
            stop_context_request(
                state,
                HostRequest::Stop {
                    session_id: previous.session_id
                }
            )
            .await?,
            HostResponse::Acknowledged
        ) {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "worker_recovery_cleanup_unconfirmed",
                "Worker engine did not acknowledge recovered-session cleanup.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    async fn serve_stop_handshake(
        listener: tokio::net::UnixListener,
        snapshot: swarm_terminal::HostSessionSummary,
        version: u16,
        expected: &[&str],
        verify: Option<(
            swarm_persistence::TaskStore,
            WorkerId,
            swarm_domain::ProviderConversationId,
        )>,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for expected in expected {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut request = String::new();
            stream.read_line(&mut request).await.unwrap();
            let request: HostRequest = serde_json::from_str(&request).unwrap();
            assert_eq!(serde_json::to_value(&request).unwrap()["type"], *expected);
            let response = match request {
                HostRequest::Ping => HostResponse::Pong {
                    protocol_version: version,
                },
                HostRequest::ListSessions => HostResponse::Sessions {
                    sessions: vec![snapshot.clone()],
                },
                HostRequest::Stop { .. } => {
                    if let Some((store, worker, conversation)) = &verify {
                        assert_eq!(
                            store
                                .get_worker_profile(*worker)
                                .unwrap()
                                .provider_conversation_id,
                            Some(*conversation),
                            "context must commit before removal"
                        );
                    }
                    HostResponse::Acknowledged
                }
                HostRequest::StopRetained { .. } => HostResponse::Acknowledged,
                _ => panic!("unexpected stop request"),
            };
            let mut response = serde_json::to_vec(&response).unwrap();
            response.push(b'\n');
            stream.get_mut().write_all(&response).await.unwrap();
        }
    }

    fn bare_session_summary(
        session_id: WorkerSessionId,
        running: bool,
    ) -> swarm_terminal::HostSessionSummary {
        swarm_terminal::HostSessionSummary {
            session_id,
            running,
            stop_pending_release: false,
            continuation_unavailable: false,
            continuation_recovery: None,
            resources: None,
            last_output_at: None,
            recovery_attempt: None,
            provider_start: None,
            provider_selection: None,
        }
    }

    #[tokio::test]
    async fn retained_stop_commits_context_before_cleanup_and_recovers_after_api_failure() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("stop.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let resumed = swarm_domain::ProviderConversationId::new();
        let snapshot = swarm_terminal::HostSessionSummary {
            session_id: session,
            running: false,
            stop_pending_release: true,
            continuation_unavailable: false,
            continuation_recovery: None,
            resources: None,
            last_output_at: None,
            recovery_attempt: None,
            provider_start: None,
            provider_selection: Some(swarm_domain::ProviderConversationSelection {
                revision: 2,
                conversation: resumed,
            }),
        };
        let unavailable = AppState::default()
            .with_terminal_host(swarm_terminal::HostClient::new(&socket), "fixture");
        let restored = AppState::default()
            .with_task_store(store.clone())
            .with_terminal_host(swarm_terminal::HostClient::new(&socket), "fixture");
        let serve = serve_stop_handshake(
            listener,
            snapshot,
            15,
            &[
                "ping",
                "stop_retained",
                "list_sessions",
                "list_sessions",
                "stop",
            ],
            Some((store.clone(), worker.id, resumed)),
        );
        let recover = async {
            assert!(
                stop_worker_session_preserving_context(&unavailable, session)
                    .await
                    .is_err()
            );
            assert_eq!(store.active_worker_sessions().unwrap().len(), 1);
            let live = reconcile_worker_bindings(&restored).await.unwrap();
            assert!(live.is_empty());
            assert!(store.active_worker_sessions().unwrap().is_empty());
            assert_eq!(
                store
                    .get_worker_profile(worker.id)
                    .unwrap()
                    .provider_conversation_id,
                Some(resumed)
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(serve, recover)
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn stop_preserves_context_and_refuses_unfrozen_or_unknown_engines() {
        for (version, frozen, succeeds) in [
            (14, false, true),
            (15, true, true),
            (15, false, false),
            (swarm_terminal::PROTOCOL_VERSION, true, true),
            (swarm_terminal::PROTOCOL_VERSION, false, false),
            (swarm_terminal::PROTOCOL_VERSION + 1, false, false),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("stop.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let store = swarm_persistence::TaskStore::in_memory().unwrap();
            let worker = store.ensure_queen("/workspace").unwrap();
            let session = WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            let resumed = swarm_domain::ProviderConversationId::new();
            let snapshot = swarm_terminal::HostSessionSummary {
                session_id: session,
                running: version == 14,
                stop_pending_release: frozen,
                continuation_unavailable: false,
                continuation_recovery: None,
                resources: None,
                last_output_at: None,
                recovery_attempt: None,
                provider_start: None,
                provider_selection: Some(swarm_domain::ProviderConversationSelection {
                    revision: 2,
                    conversation: resumed,
                }),
            };
            let state = Arc::new(
                AppState::default()
                    .with_task_store(store.clone())
                    .with_terminal_host(swarm_terminal::HostClient::new(&socket), "fixture"),
            );
            let expected: &[&str] = match (version, frozen) {
                (14, _) => &["ping", "list_sessions", "stop"],
                (version, true) if (15..=swarm_terminal::PROTOCOL_VERSION).contains(&version) => {
                    &["ping", "stop_retained", "list_sessions", "stop"]
                }
                (version, false) if (15..=swarm_terminal::PROTOCOL_VERSION).contains(&version) => {
                    &["ping", "stop_retained", "list_sessions"]
                }
                _ => &["ping"],
            };
            let serve = serve_stop_handshake(
                listener,
                snapshot,
                version,
                expected,
                Some((store.clone(), worker.id, resumed)),
            );
            let ((), stopped) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(
                    serve,
                    crate::workers::stand_worker_down(&state, worker.id, None)
                )
            })
            .await
            .unwrap();
            assert_eq!(stopped.is_ok(), succeeds);
            assert_eq!(store.active_worker_sessions().unwrap().is_empty(), succeeds);
        }
    }

    async fn serve_session_snapshot(
        listener: &tokio::net::UnixListener,
        sessions: Vec<swarm_terminal::HostSessionSummary>,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<HostRequest>(&request).unwrap(),
            HostRequest::ListSessions
        ));
        let mut response = serde_json::to_vec(&HostResponse::Sessions { sessions }).unwrap();
        response.push(b'\n');
        stream.get_mut().write_all(&response).await.unwrap();
    }

    async fn acknowledge_recovery_cleanup(
        listener: &tokio::net::UnixListener,
        store: &swarm_persistence::TaskStore,
        worker: WorkerId,
        previous: WorkerSessionId,
        successor: WorkerSessionId,
        chosen: swarm_domain::ProviderConversationId,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.unwrap();
        assert!(
            matches!(serde_json::from_str::<HostRequest>(&request).unwrap(), HostRequest::Stop { session_id } if session_id == previous)
        );
        let profile = store.get_worker_profile(worker).unwrap();
        assert_eq!(profile.active_session_id, Some(successor));
        assert_eq!(profile.provider_conversation_id, Some(chosen));
        let mut response = serde_json::to_vec(&HostResponse::Acknowledged).unwrap();
        response.push(b'\n');
        stream.get_mut().write_all(&response).await.unwrap();
    }

    #[tokio::test]
    async fn recovery_cleanup_lost_reply_preserves_successor_and_reconciles_again() {
        use swarm_terminal::{ContinuationRecoveryOutcome, HostClient};
        use tokio::io::{AsyncBufReadExt, BufReader};
        for removed in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("cleanup.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let store = swarm_persistence::TaskStore::in_memory().unwrap();
            let worker = store.ensure_queen("/workspace").unwrap();
            let chosen = swarm_domain::ProviderConversationId::new();
            store
                .repoint_provider_conversation(worker.id, &chosen)
                .unwrap();
            let previous = WorkerSessionId::new();
            let successor = WorkerSessionId::new();
            let (continuation, fresh) = continuation_and_fresh_attempts();
            store.bind_worker_session(worker.id, previous).unwrap();
            assert!(
                store
                    .reconcile_continuation_successor(previous, continuation, successor, fresh)
                    .unwrap()
            );
            let parent = swarm_terminal::HostSessionSummary {
                recovery_attempt: Some(continuation),
                continuation_recovery: Some(ContinuationRecoveryOutcome::SessionCreated {
                    session_id: successor,
                }),
                ..bare_session_summary(previous, false)
            };
            let child = swarm_terminal::HostSessionSummary {
                recovery_attempt: Some(fresh),
                ..bare_session_summary(successor, true)
            };
            let state = AppState::default()
                .with_task_store(store.clone())
                .with_terminal_host(HostClient::new(&socket), "fixture");
            let server = async {
                serve_session_snapshot(&listener, vec![parent.clone(), child.clone()]).await;
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                let mut request = String::new();
                stream.read_line(&mut request).await.unwrap();
                assert!(
                    matches!(serde_json::from_str::<HostRequest>(&request).unwrap(), HostRequest::Stop { session_id } if session_id == previous)
                );
                // Drop the connection without any response, both when cleanup
                // happened and when the engine retained the parent.
            };
            let ((), first) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(server, reconcile_worker_bindings(&state))
            })
            .await
            .unwrap();
            assert!(first.is_err());
            assert_eq!(
                store
                    .get_worker_profile(worker.id)
                    .unwrap()
                    .active_session_id,
                Some(successor)
            );
            let server = async {
                let snapshots = if removed {
                    vec![child]
                } else {
                    vec![parent, child]
                };
                serve_session_snapshot(&listener, snapshots).await;
                if !removed {
                    acknowledge_recovery_cleanup(
                        &listener, &store, worker.id, previous, successor, chosen,
                    )
                    .await;
                }
            };
            let replacement_api = AppState::default()
                .with_task_store(store.clone())
                .with_terminal_host(HostClient::new(&socket), "replacement");
            let ((), second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(server, reconcile_worker_bindings(&replacement_api))
            })
            .await
            .unwrap();
            assert!(second.unwrap().contains_key(&successor));
            assert_eq!(
                store
                    .get_worker_profile(worker.id)
                    .unwrap()
                    .active_session_id,
                Some(successor)
            );
        }
    }

    #[tokio::test]
    async fn successor_reconciliation_precedes_startup_and_dead_binding_release() {
        use swarm_domain::{
            ConversationRecoveryState, ProviderConversationId, ProviderSessionStartKind,
        };
        use swarm_terminal::{
            ContinuationRecoveryOutcome, HostClient, ProviderSessionStartObservation,
        };
        for scenario in ["normal", "reopened", "manual", "missing", "wrong_attempt"] {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("successor.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let store = swarm_persistence::TaskStore::in_memory().unwrap();
            let worker = store.ensure_queen("/workspace").unwrap();
            let previous = WorkerSessionId::new();
            let successor = WorkerSessionId::new();
            let (continuation, fresh) = continuation_and_fresh_attempts();
            store.bind_worker_session(worker.id, previous).unwrap();
            if scenario == "reopened" {
                assert!(
                    store
                        .reconcile_continuation_successor(previous, continuation, successor, fresh)
                        .unwrap()
                );
            }
            let chosen = ProviderConversationId::new();
            if scenario == "manual" {
                store
                    .repoint_provider_conversation(worker.id, &chosen)
                    .unwrap();
            }
            let mut summaries = vec![swarm_terminal::HostSessionSummary {
                recovery_attempt: Some(continuation),
                continuation_recovery: Some(ContinuationRecoveryOutcome::SessionCreated {
                    session_id: successor,
                }),
                ..bare_session_summary(previous, false)
            }];
            if scenario != "missing" {
                summaries.insert(
                    0,
                    swarm_terminal::HostSessionSummary {
                        recovery_attempt: Some(if scenario == "wrong_attempt" {
                            continuation
                        } else {
                            fresh
                        }),
                        provider_start: Some(ProviderSessionStartObservation {
                            conversation: chosen,
                            kind: ProviderSessionStartKind::New,
                        }),
                        ..bare_session_summary(successor, true)
                    },
                );
            }
            let state = AppState::default()
                .with_task_store(store.clone())
                .with_terminal_host(HostClient::new(&socket), "fixture");
            let ((), result) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(
                    async {
                        serve_session_snapshot(&listener, summaries).await;
                        if matches!(scenario, "normal" | "reopened") {
                            acknowledge_recovery_cleanup(
                                &listener, &store, worker.id, previous, successor, chosen,
                            )
                            .await;
                        }
                    },
                    reconcile_worker_bindings(&state)
                )
            })
            .await
            .unwrap();
            let live = result.unwrap();
            assert_eq!(live.contains_key(&successor), scenario != "missing");
            let profile = store.get_worker_profile(worker.id).unwrap();
            if matches!(scenario, "normal" | "reopened") {
                assert_eq!(profile.active_session_id, Some(successor));
                assert_eq!(profile.provider_conversation_id, Some(chosen));
                assert!(matches!(
                    store
                        .provider_recovery_outcomes(&[successor])
                        .unwrap()
                        .get(&successor),
                    Some(ConversationRecoveryState::Fresh { .. })
                ));
            } else {
                assert_eq!(profile.active_session_id, None);
                if scenario == "manual" {
                    assert_eq!(profile.provider_conversation_id, Some(chosen));
                }
            }
        }
    }

    fn continuation_and_fresh_attempts() -> (
        swarm_domain::ConversationRecoveryAttempt,
        swarm_domain::ConversationRecoveryAttempt,
    ) {
        use swarm_domain::{
            ConversationRecovery, ConversationRecoveryEvidence, ConversationRecoveryState,
        };
        let mut recovery = ConversationRecovery::new(None, true);
        let ConversationRecoveryState::Attempt {
            attempt: continuation,
        } = recovery.state()
        else {
            panic!("continue");
        };
        recovery.observe(
            continuation,
            ConversationRecoveryEvidence::ContextUnavailable,
        );
        let ConversationRecoveryState::Attempt { attempt: fresh } = recovery.state() else {
            panic!("fresh");
        };
        (continuation, fresh)
    }

    #[tokio::test]
    async fn exited_provider_evidence_is_saved_before_release_but_cannot_override_a_new_binding() {
        use swarm_domain::{
            ConversationRecovery, ConversationRecoveryEvidence, ConversationRecoveryState,
            ProviderConversationId, ProviderConversationSelection, ProviderSessionStartKind,
        };
        use swarm_terminal::{HostClient, HostSessionSummary, ProviderSessionStartObservation};

        for scenario in ["startup", "selection", "manual", "replacement"] {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("host.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let store = swarm_persistence::TaskStore::in_memory().unwrap();
            let worker = store.ensure_queen("/workspace").unwrap();
            let original = ProviderConversationId::new();
            let resumed = ProviderConversationId::new();
            let manual = ProviderConversationId::new();
            store
                .repoint_provider_conversation(worker.id, &original)
                .unwrap();
            let session = WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            let mut recovery = ConversationRecovery::new(Some(original), true);
            let ConversationRecoveryState::Attempt { attempt } = recovery.state() else {
                panic!("expected exact attempt");
            };
            recovery.observe(attempt, ConversationRecoveryEvidence::ContextUnavailable);
            let ConversationRecoveryState::Attempt { attempt } = recovery.state() else {
                panic!("expected continuation");
            };
            let mut summaries = vec![HostSessionSummary {
                recovery_attempt: Some(attempt),
                provider_start: (scenario != "selection").then_some(
                    ProviderSessionStartObservation {
                        conversation: resumed,
                        kind: ProviderSessionStartKind::Resumed,
                    },
                ),
                provider_selection: (scenario != "startup").then_some(
                    ProviderConversationSelection {
                        revision: 2,
                        conversation: resumed,
                    },
                ),
                ..bare_session_summary(session, false)
            }];
            if scenario == "manual" {
                store
                    .repoint_provider_conversation(worker.id, &manual)
                    .unwrap();
            }
            let replacement = WorkerSessionId::new();
            if scenario == "replacement" {
                store.release_worker_session(session).unwrap();
                store.bind_worker_session(worker.id, replacement).unwrap();
                summaries.push(bare_session_summary(replacement, true));
            }
            let state = AppState::default()
                .with_task_store(store.clone())
                .with_terminal_host(HostClient::new(socket), "fixture-operator");
            let serve = serve_session_snapshot(&listener, summaries);
            let ((), reconciled) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(serve, reconcile_worker_bindings(&state))
            })
            .await
            .unwrap();
            let live = reconciled.unwrap();
            assert!(!live.contains_key(&session));
            assert_eq!(live.contains_key(&replacement), scenario == "replacement");
            let expected = match scenario {
                "manual" => manual,
                "replacement" => original,
                _ => resumed,
            };
            assert_eq!(
                store
                    .get_worker_profile(worker.id)
                    .unwrap()
                    .provider_conversation_id,
                Some(expected),
                "{scenario}"
            );
            assert_eq!(
                store.active_worker_sessions().unwrap().len(),
                usize::from(scenario == "replacement")
            );
        }
    }

    #[test]
    fn codex_saved_conversation_wins_over_session_history() {
        use swarm_terminal::{CodexConversationStart, HostRequest, TerminalSize};

        let root = tempfile::tempdir().unwrap();
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let mut profile = store
            .create_worker(
                "Codex",
                swarm_domain::ProviderKind::Codex,
                &root.path().to_string_lossy(),
                false,
                1,
            )
            .unwrap();
        let state = crate::AppState::default()
            .with_workspace_roots(vec![root.path().to_path_buf()])
            .with_task_store(store);
        let chosen = "22222222-2222-4222-8222-222222222222".parse().unwrap();
        for (identity, history, expected) in [
            (
                Some(chosen),
                false,
                CodexConversationStart::Resume { session_id: chosen },
            ),
            (
                Some(chosen),
                true,
                CodexConversationStart::Resume { session_id: chosen },
            ),
            (None, true, CodexConversationStart::Continue),
            (None, false, CodexConversationStart::New),
        ] {
            profile.provider_conversation_id = identity;
            profile.has_session_history = history;
            let request = super::provider_start_request(
                &state,
                profile.id,
                &profile,
                TerminalSize::default(),
                None,
            )
            .unwrap();
            let HostRequest::StartCodex { conversation, .. } = request else {
                panic!("Codex must not switch providers");
            };
            assert_eq!(conversation, expected);
        }
    }

    use super::*;

    #[tokio::test]
    async fn revival_rechecks_cancellation_and_policy_before_host_contact() {
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Experimental", ProviderKind::Gemini, "/workspace", false, 1)
            .unwrap();
        let state = AppState::default().with_task_store(store.clone());
        store
            .record_worker_revival_intents(&[worker.id], 1)
            .unwrap();
        store
            .set_manual_presence(
                Some(swarm_domain::PresenceMode::NightWatch),
                crate::unix_timestamp(),
            )
            .unwrap();
        assert!(
            revive_worker_process(&state, worker.id, TerminalSize::default())
                .await
                .unwrap()
                .is_none()
        );
        assert!(store.worker_revival_pending(worker.id).unwrap());
        assert!(state.worker_errors.read().await.is_empty());
        assert!(state.worker_recovery_attempts.read().await.is_empty());
        store
            .set_manual_presence(
                Some(swarm_domain::PresenceMode::AtHive),
                crate::unix_timestamp(),
            )
            .unwrap();
        store.clear_worker_revival_intent(worker.id).unwrap();
        assert!(
            revive_worker_process(&state, worker.id, TerminalSize::default())
                .await
                .unwrap()
                .is_none()
        );
    }

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
        let freshness = conversation_freshness(
            &profile,
            &projects,
            home.path(),
            &mut ConversationScanBudget::new(),
            &HashMap::new(),
        );

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

    #[test]
    fn scan_limits_never_publish_partial_health() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("projects/scout");
        let projects = home.path().join(".claude/projects");
        let slug = workspace.to_string_lossy().replace(['/', '.'], "-");
        let id = "11111111-1111-4111-8111-111111111111";
        transcript(&projects.join(&slug), id, "2026-09-02T09:00:00.000Z");
        let profile = worker_profile(&workspace, Some(id), true);
        for kind in ["entries", "bytes", "deadline"] {
            let mut budget = ConversationScanBudget::new();
            match kind {
                "entries" => budget.entries = 1,
                "bytes" => budget.bytes = 1,
                _ => budget.deadline = std::time::Instant::now(),
            }
            assert!(matches!(
                conversation_freshness(
                    &profile,
                    &projects,
                    home.path(),
                    &mut budget,
                    &HashMap::new()
                ),
                ConversationFreshness::Unknown { .. }
            ));
            assert!(
                budget.exhausted,
                "{kind} limit must be visible to the whole-scan owner"
            );
            assert!(!budget.reserve(0, 0));
        }
    }

    #[test]
    fn confirmed_current_selection_outweighs_newer_transcript_but_not_a_changed_binding() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("projects/scout");
        let projects = home.path().join(".claude/projects");
        let directory = projects.join(workspace.to_string_lossy().replace(['/', '.'], "-"));
        let chosen = "11111111-1111-4111-8111-111111111111";
        transcript(&directory, chosen, "2026-09-02T00:00:00.000Z");
        transcript(
            &directory,
            "22222222-2222-4222-8222-222222222222",
            "2026-09-02T09:00:00.000Z",
        );
        let mut profile = worker_profile(&workspace, Some(chosen), true);
        let session = WorkerSessionId::new();
        profile.active_session_id = Some(session);
        let confirmed = HashMap::from([(
            session,
            swarm_domain::ProviderConversationSelection {
                revision: 2,
                conversation: profile.provider_conversation_id.unwrap(),
            },
        )]);
        let mut budget = ConversationScanBudget::new();
        budget.entries = 0;
        assert_eq!(
            conversation_freshness(&profile, &projects, home.path(), &mut budget, &confirmed),
            ConversationFreshness::Current
        );
        assert!(
            !budget.exhausted,
            "confirmed selection does not scan transcript files"
        );

        profile.active_session_id = Some(WorkerSessionId::new());
        assert!(matches!(
            conversation_freshness(
                &profile,
                &projects,
                home.path(),
                &mut ConversationScanBudget::new(),
                &confirmed
            ),
            ConversationFreshness::Stale { .. }
        ));
        profile.active_session_id = Some(session);
        profile.provider_conversation_id = Some(swarm_domain::ProviderConversationId::new());
        assert!(matches!(
            conversation_freshness(
                &profile,
                &projects,
                home.path(),
                &mut ConversationScanBudget::new(),
                &confirmed
            ),
            ConversationFreshness::Stale { .. }
        ));
        profile.active_session_id = None;
        assert!(matches!(
            conversation_freshness(
                &profile,
                &projects,
                home.path(),
                &mut ConversationScanBudget::new(),
                &confirmed
            ),
            ConversationFreshness::Stale { .. }
        ));
    }

    #[test]
    fn transcript_reader_does_not_follow_symlinks_or_open_fifo_streams() {
        let directory = tempfile::tempdir().unwrap();
        transcript(directory.path(), "regular", "2026-09-02T09:00:00.000Z");
        let link = directory.path().join("link.jsonl");
        std::os::unix::fs::symlink(directory.path().join("regular.jsonl"), &link).unwrap();
        let fifo = directory.path().join("fifo.jsonl");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
        let mut budget = ConversationScanBudget::new();
        assert!(last_entry_timestamp(&link, &mut budget).is_none());
        assert!(last_entry_timestamp(&fifo, &mut budget).is_none());
        assert!(!budget.exhausted);
    }

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
            conversation_freshness(
                &profile,
                &projects,
                home.path(),
                &mut ConversationScanBudget::new(),
                &HashMap::new(),
            ),
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
        let freshness = conversation_freshness(
            &profile,
            &projects,
            home.path(),
            &mut ConversationScanBudget::new(),
            &HashMap::new(),
        );

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
