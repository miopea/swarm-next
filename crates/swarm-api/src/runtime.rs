use std::{collections::HashMap, path::Path as FilePath, process::Command, sync::Arc};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use swarm_terminal::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, HostRequest, HostResponse,
    MAX_CANONICAL_SNAPSHOT_BYTES, MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS,
    ProcessResourceSample, sample_current_process,
};

use crate::attach::MAX_ATTACH_GRANTS;
use crate::{
    ApiError, AppState, MAX_TERMINAL_WEBSOCKETS, authorize, build_version,
    terminal_host::authorized_no_store_request, unix_timestamp,
};

#[derive(Debug, Serialize)]
pub(super) struct RuntimeLimitsResponse {
    terminal: TerminalRuntimeLimits,
}

#[derive(Debug, Serialize)]
struct TerminalRuntimeLimits {
    journal_max_bytes: usize,
    journal_max_frames: usize,
    attach_grant_max_active: usize,
    websocket_max_active: usize,
    canonical_scrollback_rows: usize,
    canonical_compaction_input_bytes: usize,
    canonical_snapshot_max_bytes: usize,
    max_rows: u16,
    max_columns: u16,
    max_cells: usize,
    /// Published so the control room refuses an oversized image with the real
    /// number rather than a copy of it that can drift.
    attachment_max_bytes: usize,
}

/// Whether the terminal host is behind the API that is answering.
///
/// ASKED OF THE RUNNING HOST, not of a symlink. A symlink comparison reported
/// "already uses 0.8.17-dev-68f517e" on 2026-08-27 while the host process and
/// all fourteen terminals were still executing a two-release-old image whose
/// directory had been deleted. A check that cannot see the thing it checks
/// reports agreement.
///
/// A host that cannot be reached answers None rather than false: not knowing
/// and being up to date are different facts, and collapsing them is how a
/// staleness check becomes a check that cannot fail.
async fn engine_update_required(state: &Arc<AppState>) -> (Option<bool>, Option<usize>) {
    let Ok(status) = crate::maintenance::host_status_snapshot(state).await else {
        return (None, None);
    };
    // BOTH FACTS FROM ONE SNAPSHOT. The session count is what turns "the engine
    // is behind" into a sentence an operator can weigh: a swap that stops eleven
    // sessions and a swap that stops none read identically without it, and the
    // difference is the whole cost. Asking the host twice for one screen would
    // also let the two answers disagree.
    (
        Some(crate::maintenance::worker_engine_update_required(&status)),
        Some(status.running_sessions),
    )
}

/// Whether the CHECKOUT changes the terminal-host protocol.
///
/// SEPARATE FROM `engine_update_required`, AND THIS IS THE CASE THAT WAS
/// INVISIBLE. That one compares the RUNNING host against the RUNNING API, and
/// both are the build in service — so a checkout that bumps `PROTOCOL_VERSION`
/// leaves them agreeing with each other and reports nothing. The operator only
/// found out by pressing reload and being refused.
///
/// A protocol change is the one thing a reload cannot install. The reload
/// deliberately leaves the terminal host running, which is what keeps worker
/// terminals alive across it, so installing a new protocol means swapping both
/// processes together and stopping every worker to do it. That is a decision
/// about timing, and an operator cannot make it if nothing tells them it is
/// pending.
///
/// Read from the checkout with the same expression `build-development-release.sh`
/// uses, so the number reported here is the number the build would stamp.
async fn protocol_migration_required(state: &Arc<AppState>) -> Option<bool> {
    let checkout = state.development_checkout_path.as_ref()?;
    let source = std::fs::read_to_string(checkout.join("crates/swarm-terminal/src/ipc.rs")).ok()?;
    let declared: u16 = source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("pub const PROTOCOL_VERSION: u16 = ")?
            .strip_suffix(';')?
            .parse()
            .ok()
    })?;
    let running = crate::maintenance::host_status_snapshot(state).await.ok()?;
    Some(declared != running.protocol_version)
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a response shape: each flag is an independent fact the reload card renders"
)]
struct DevelopmentRuntimeResponse {
    enabled: bool,
    version: &'static str,
    state: &'static str,
    reload_available: bool,
    deployed_source_revision: Option<String>,
    source_revision: Option<String>,
    source_dirty: bool,
    /// Whether the running revision exists on a remote, so the operator can see
    /// that their Hive is on code that lives only on this machine.
    deployed_source_published: bool,
    /// Whether the worker engine is behind the running API.
    ///
    /// "IT IS LIVE" MEANS FOUR DIFFERENT THINGS HERE and this is the second of
    /// them. The terminal host is a separate service that deliberately survives
    /// a reload so worker terminals are not killed mid-turn, so a reload can
    /// leave the engine behind — and that was discoverable only by something
    /// not working. The operator met it as `Runtime request returned 422:
    /// unknown variant "start_shell"`, which reads as a protocol bug rather
    /// than as a service that had not restarted.
    ///
    /// Reported here because this is the status an operator is already watching
    /// when they reload, rather than in a document they would have to know to
    /// go and read.
    ///
    /// None means the host could not be asked. Absent is not the same claim as
    /// up to date, and saying "current" because nothing answered is the failure
    /// this Hive keeps removing.
    worker_engine_update_required: Option<bool>,
    /// How many worker sessions are running, from the same host snapshot.
    ///
    /// A PLAIN COUNT, not a judgement. The card decides how to phrase what it
    /// costs; this only says how many there are, so the number cannot drift from
    /// the staleness flag it is shown beside. None means the host could not be
    /// asked, which is the same silence the flag itself reports.
    running_worker_sessions: Option<usize>,
    /// Whether the checkout changes the terminal-host protocol, which a reload
    /// cannot install. None means the host could not be asked, or there is no
    /// development checkout to compare against.
    protocol_migration_required: Option<bool>,
    /// Which step failed: `build`, `install`, `protocol-change`, or absent.
    ///
    /// Every failure used to reach the operator as "the working copy did not
    /// compile", whatever had happened — so an install refused for a good
    /// reason read as a compiler error and the only way to learn otherwise was
    /// the journal.
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason: Option<String>,
    /// The failing step's own last words, one bounded line.
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct RuntimeResourcesResponse {
    sampled_at: i64,
    policy: ResourcePolicyResponse,
    api: ProcessResourceResponse,
    terminal_host: ProcessResourceResponse,
    machine: MachineResourceResponse,
    daily_backup: crate::backups::DailyBackupStatus,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct MachineResourceResponse {
    memory_total_bytes: Option<u64>,
    memory_available_bytes: Option<u64>,
    memory_used_percent: Option<f64>,
    swap_total_bytes: Option<u64>,
    swap_used_bytes: Option<u64>,
    swap_used_percent: Option<f64>,
    load_average: Option<[f64; 3]>,
    logical_cpus: Option<usize>,
    memory_pressure_avg10: Option<f64>,
    cpu_pressure_avg10: Option<f64>,
    io_pressure_avg10: Option<f64>,
    pressure: ResourcePressure,
}

/// A machine of a stated size and verdict, so tests can place a layer against
/// one. The fields stay private: what a test needs to say is "a big idle
/// machine" or "a small stalling one", not eleven readings.
#[cfg(test)]
pub(super) fn machine_of(total_bytes: u64, pressure: ResourcePressure) -> MachineResourceResponse {
    MachineResourceResponse {
        memory_total_bytes: Some(total_bytes),
        pressure,
        ..Default::default()
    }
}

#[derive(Debug, Serialize)]
struct ResourcePolicyResponse {
    mode: &'static str,
    /// Percentage of the machine's memory at which a layer becomes worth
    /// naming, and at which it becomes the thing to look at. A share of the
    /// machine rather than a byte ceiling: a fixed 512 MiB ceiling called ten
    /// healthy workers Critical on a 32 GiB machine that was not stalling.
    advisory_percent: u64,
    critical_percent: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResourcePressure {
    #[default]
    Normal,
    Advisory,
    Critical,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CoordinatorStartAdmission {
    Allowed,
    DeferredAdvisory,
    DeferredCritical,
    DeferredUnavailable,
}

impl CoordinatorStartAdmission {
    pub(super) const fn code(self) -> u8 {
        match self {
            Self::Allowed => 0,
            Self::DeferredAdvisory => 1,
            Self::DeferredCritical => 2,
            Self::DeferredUnavailable => 3,
        }
    }

    pub(super) const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Allowed,
            1 => Self::DeferredAdvisory,
            2 => Self::DeferredCritical,
            _ => Self::DeferredUnavailable,
        }
    }

    pub(super) const fn permits_start(self) -> bool {
        matches!(self, Self::Allowed)
    }

    /// Why a start was refused, in the operator's terms rather than the
    /// admission code's.
    ///
    /// The refusal already reached them — it becomes a held-delivery card and
    /// escalates after twelve hours — but it only said the Hive "is not
    /// currently admitted to start it", which names the mechanism and not the
    /// cause. An operator reading that cannot tell a machine under memory
    /// pressure from a broken coordinator, and those want opposite responses:
    /// one is "close something", the other is "come and look at Swarm".
    ///
    /// This is the second place machine pressure reaches the operator, and it
    /// is deliberately the only one besides the header. It fires on a
    /// CONSEQUENCE — work that could not be delivered — rather than on a gauge
    /// crossing a line, so it cannot cry wolf while the machine is merely busy
    /// and nothing is waiting.
    pub(super) const fn refusal_reason(self) -> &'static str {
        match self {
            // Not a refusal at all; present so the match stays total if a
            // caller ever asks about an admitted start.
            Self::Allowed => "this Hive is admitted to start workers",
            Self::DeferredAdvisory => {
                "this machine is under memory pressure, so Swarm is not starting more workers until it eases"
            }
            Self::DeferredCritical => {
                "this machine is critically short of memory, so Swarm has stopped starting workers to avoid taking the machine down"
            }
            Self::DeferredUnavailable => {
                "Swarm cannot read this machine's resources, so it will not start more workers while it cannot tell whether that is safe"
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ProcessResourceResponse {
    resident_memory_bytes: Option<u64>,
    process_tree_resident_memory_bytes: Option<u64>,
    process_tree_process_count: Option<u32>,
    pub(super) pressure: ResourcePressure,
}

pub(super) async fn limits(State(state): State<Arc<AppState>>) -> Json<RuntimeLimitsResponse> {
    Json(RuntimeLimitsResponse {
        terminal: TerminalRuntimeLimits {
            journal_max_bytes: state.terminal_limits.max_bytes,
            journal_max_frames: state.terminal_limits.max_frames,
            attach_grant_max_active: MAX_ATTACH_GRANTS,
            websocket_max_active: MAX_TERMINAL_WEBSOCKETS,
            canonical_scrollback_rows: CANONICAL_SCROLLBACK_ROWS,
            canonical_compaction_input_bytes: CANONICAL_COMPACTION_INPUT_BYTES,
            canonical_snapshot_max_bytes: MAX_CANONICAL_SNAPSHOT_BYTES,
            max_rows: MAX_TERMINAL_ROWS,
            max_columns: MAX_TERMINAL_COLUMNS,
            max_cells: MAX_TERMINAL_CELLS,
            attachment_max_bytes: crate::attachments::MAX_ATTACHMENT_BYTES,
        },
    })
}

pub(super) async fn terminal_host_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized_no_store_request(&state, &headers, HostRequest::HostStatus).await
}

pub(super) async fn development(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let source = development_source_status(&state);
    // One host snapshot serves both the staleness flag and the session count, so
    // the two cannot disagree on the screen that shows them together.
    let engine_update = engine_update_required(&state).await;
    let source_aligned = source.as_ref().is_some_and(|status| status.aligned);
    let state_name = if source.is_some() && !source_aligned {
        "source_mismatch"
    } else {
        development_reload_state_for_source(
            &state,
            source.as_ref().map(|status| status.revision.as_str()),
        )
    };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(DevelopmentRuntimeResponse {
            enabled: state.development_reload_request_path.is_some(),
            version: build_version(),
            state: state_name,
            reload_available: source
                .as_ref()
                .is_some_and(|status| status.reload_available),
            deployed_source_revision: build_source_revision(),
            source_revision: source.as_ref().map(|status| status.revision.clone()),
            source_dirty: source.as_ref().is_some_and(|status| status.dirty),
            deployed_source_published: source.is_some_and(|status| status.published),
            worker_engine_update_required: engine_update.0,
            running_worker_sessions: engine_update.1,
            protocol_migration_required: protocol_migration_required(&state).await,
            failure_reason: development_status_field(&state, "reason="),
            failure_detail: development_status_field(&state, "detail="),
        }),
    )
        .into_response())
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent facts about one checkout; collapsing them would hide which is which"
)]
pub(super) struct DevelopmentSourceStatus {
    pub(super) revision: String,
    pub(super) dirty: bool,
    pub(super) reload_available: bool,
    pub(super) aligned: bool,
    /// Whether the RUNNING revision exists on a remote, and so survives losing
    /// this machine.
    ///
    /// A reload builds from the local checkout, which is what makes the
    /// develop-and-reload loop work and is right for a tool whose developer is
    /// its operator. What was invisible is the consequence: on 2026-08-25 this
    /// Hive ran a commit that existed on no remote for about twenty minutes,
    /// and nothing said so. Both a worker and Queen reported that work as
    /// "pushed, not deployed" when it was in fact deployed and not pushed —
    /// committed, pushed and deployed are three claims, and the surface only
    /// carried the third.
    ///
    /// FALSE WHEN UNKNOWN, deliberately. A wrong "unpushed" costs a glance; a
    /// wrong "published" costs the code if the machine is lost. The cheap error
    /// is the one to make.
    pub(super) published: bool,
}

pub(super) fn development_source_status(state: &AppState) -> Option<DevelopmentSourceStatus> {
    let checkout = state.development_checkout_path.as_ref()?;
    development_source_status_for(checkout, build_source_revision().as_deref())
}

pub(super) fn development_source_status_for(
    checkout: &FilePath,
    deployed_revision: Option<&str>,
) -> Option<DevelopmentSourceStatus> {
    let revision = git_output(checkout, &["rev-parse", "--short=12", "HEAD"])?;
    let dirty = !git_output_with_paths(
        checkout,
        &["status", "--porcelain", "--untracked-files=normal", "--"],
        &DEVELOPMENT_PRODUCT_PATHS,
    )
    .is_some_and(|output| output.is_empty());
    let aligned = deployed_revision.is_some_and(|deployed| {
        Command::new("git")
            .arg("-C")
            .arg(checkout)
            .args(["merge-base", "--is-ancestor", deployed, "HEAD"])
            .status()
            .is_ok_and(|status| status.success())
    });
    let committed_changes = aligned
        && deployed_revision.is_some_and(|deployed| {
            Command::new("git")
                .arg("-C")
                .arg(checkout)
                .args(["diff", "--quiet", deployed, "HEAD", "--"])
                .args(DEVELOPMENT_PRODUCT_PATHS)
                .status()
                .is_ok_and(|status| !status.success())
        });
    // ANY remote ref, not origin/main. The property that matters is whether
    // this commit survives losing the machine, and a commit pushed to a feature
    // branch is exactly as recoverable as one on main. Asking about main would
    // report perfectly safe work as at risk.
    //
    // Remote-tracking refs can be stale if nothing has fetched, which can only
    // make this answer "not published" for something that is — the safe
    // direction.
    let published = deployed_revision.is_some_and(|deployed| {
        git_output(checkout, &["branch", "--remotes", "--contains", deployed])
            .is_some_and(|refs| !refs.trim().is_empty())
    });
    Some(DevelopmentSourceStatus {
        revision,
        dirty,
        reload_available: aligned && (dirty || committed_changes),
        aligned,
        published,
    })
}

const DEVELOPMENT_PRODUCT_PATHS: [&str; 5] =
    ["Cargo.toml", "Cargo.lock", "crates", "web", "packaging"];

pub(super) fn git_output(checkout: &FilePath, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_output_with_paths(
    checkout: &FilePath,
    arguments: &[&str],
    paths: &[&str],
) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(arguments)
        .args(paths)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) fn deployed_source_revision(version: &str) -> Option<String> {
    version
        .split('-')
        .find(|part| part.len() == 12 && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
}

pub(super) fn build_source_revision() -> Option<String> {
    option_env!("SWARM_BUILD_SOURCE_REVISION")
        .map(str::to_owned)
        .or_else(|| deployed_source_revision(build_version()))
}

/// Longer than a release build of this workspace takes, by a wide margin. A
/// build that has made no progress in this long has stopped, whether it failed,
/// was never picked up, or its watcher is not running.
const STALLED_BUILD_AFTER: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// One field from the reload status file, when it says anything.
///
/// The file is key=value lines written by `swarm-package`. An empty value is
/// treated as absent: "the step recorded nothing" and "there is no step" should
/// both render as no detail rather than as an empty quote.
fn development_status_field(state: &AppState, prefix: &str) -> Option<String> {
    let path = state.development_reload_status_path.as_ref()?;
    let value = std::fs::read_to_string(path.as_ref()).ok()?;
    value
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|found| !found.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn development_reload_state_for_source(
    state: &AppState,
    source_revision: Option<&str>,
) -> &'static str {
    let Some(path) = &state.development_reload_status_path else {
        return "disabled";
    };
    let Ok(value) = std::fs::read_to_string(path.as_ref()) else {
        // A file that cannot be read is not the same as one saying nothing is
        // happening. When its directory is not there either, development mode
        // is pointing somewhere that does not exist — which is what a migrated
        // install looked like, reporting idle while every build request went to
        // a path nobody was watching.
        let configured = std::path::Path::new(path.as_ref())
            .parent()
            .is_some_and(std::path::Path::is_dir);
        return if configured { "idle" } else { "unavailable" };
    };
    // A build reports progress by rewriting this file. One that has not been
    // touched for longer than any build takes is not in progress; nothing is
    // acting on it, and saying "building" forever is worse than saying so.
    let stalled = std::fs::metadata(path.as_ref())
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|since| since > STALLED_BUILD_AFTER);
    reload_state_from(
        &value,
        source_revision,
        build_source_revision().as_deref(),
        stalled,
    )
}

/// The rule, separated so it can be tested against this function rather than
/// against a copy of it.
///
/// `deployed_revision` is what this binary was built from. It is what makes a
/// superseded failure distinguishable from a live one, and it is the reason
/// this takes four arguments rather than reading the world itself.
fn reload_state_from(
    value: &str,
    source_revision: Option<&str>,
    deployed_revision: Option<&str>,
    stalled: bool,
) -> &'static str {
    let marker_revision = value
        .lines()
        .find_map(|line| line.strip_prefix("revision="));
    let marker_state = value.lines().find_map(|line| line.strip_prefix("state="));
    if matches!(marker_state, Some("requested" | "building" | "failed"))
        && marker_revision != source_revision
    {
        return "idle";
    }
    // A FAILURE THAT NAMES THE REVISION NOW RUNNING HAS BEEN OVERTAKEN BY EVENTS.
    //
    // The check above only forgets a stale status when it names a DIFFERENT
    // revision from the checkout. That misses the case an operator actually
    // hits: a reload fails, the revision is installed another way, and the card
    // still reports a failure for code that is now serving the page it is drawn
    // on. On 2026-08-28 a reload was refused for a protocol change, the
    // migration installed that very revision minutes later, and the control
    // room went on offering a build that was already running.
    //
    // Compared against the DEPLOYED revision rather than the checkout, because
    // "this failed" and "this is what you are running" cannot both be useful
    // and the second one wins.
    // BOTH must be known. `None == None` would make every failure that
    // recorded no revision, on a Hive that cannot say what it was built from,
    // look superseded — swallowing exactly the failures with the least
    // information attached to them.
    if marker_state == Some("failed")
        && let Some(deployed) = deployed_revision
        && marker_revision == Some(deployed)
    {
        return "idle";
    }
    match marker_state {
        Some("requested" | "building") if stalled => "stalled",
        Some("requested") => "requested",
        Some("building") => "building",
        Some("failed") => "failed",
        Some("ready") => "ready",
        _ => "idle",
    }
}

pub(super) async fn resources(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let machine = sample_machine_resources();
    let terminal_host = if let Some(client) = &state.terminal_host {
        match client.request(&HostRequest::HostStatus).await {
            Ok(HostResponse::HostStatus { status }) => {
                resource_response(status.resources, &machine)
            }
            Ok(_) | Err(_) => resource_response(None, &machine),
        }
    } else {
        resource_response(None, &machine)
    };
    let response = RuntimeResourcesResponse {
        sampled_at: unix_timestamp(),
        policy: ResourcePolicyResponse {
            mode: "observe_only",
            advisory_percent: LAYER_ADVISORY_PERCENT,
            critical_percent: LAYER_CRITICAL_PERCENT,
        },
        api: resource_response(Some(sample_current_process()), &machine),
        terminal_host,
        machine,
        daily_backup: crate::backups::daily_status(
            state
                .database_directory
                .as_deref()
                .map(std::path::PathBuf::as_path),
        )
        .await,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

/// Runnable work per core before a machine is called busy, and before it is
/// called critical.
///
/// LOAD, NOT PSI, and that is the opposite of the memory case. `/proc/pressure/cpu`
/// looked like the better signal — it measures stall directly — but measured on
/// this Hive it flaps: 9.54 to 1.23 to 3.28 inside one minute while the machine's
/// actual state changed only gradually. Worse, an IDLE four-core box here sits at
/// 1.27-2.62, so memory's Advisory threshold of 2.0 would fire on a quiet
/// machine, which is precisely the alarm-nobody-reads failure this exists to
/// avoid. `load1` over the same windows moved smoothly and tracked the real
/// condition; it is already a one-minute average, so it resists that oscillation
/// by construction.
///
/// DO NOT REACH FOR `cpu_full_avg10`. It is the variant an implementer would
/// reasonably pick as the stronger signal, the way `full` is stronger for memory.
/// Linux never reports it for CPU — something is always runnable — so it reads
/// 0.00 even at load 6.76 on four cores. A check written against it can never
/// fire.
///
/// The numbers: measured idle on this Hive over four minutes, load-per-CPU held
/// 0.25-0.37. Advisory at 1.0 is where every core has work queued behind it, and
/// leaves nearly three times the observed idle band as headroom. Critical at 2.0
/// is twice oversubscribed. The two saturation events on record land at 1.27 and
/// 1.69, with a peak of 2.46 — Advisory, rising to Critical at the peak, which is
/// the shape wanted: slow enough to say so, and only critical when it is.
const CPU_ADVISORY_LOAD_PER_CPU: f64 = 1.0;
const CPU_CRITICAL_LOAD_PER_CPU: f64 = 2.0;

fn cpu_pressure(load_average: Option<[f64; 3]>, logical_cpus: Option<usize>) -> ResourcePressure {
    // Both are needed: a load figure without a core count says nothing, because
    // load 4 is idle on 8 cores and desperate on 1.
    let (Some(load), Some(cpus)) = (load_average, logical_cpus) else {
        return ResourcePressure::Unavailable;
    };
    if cpus == 0 {
        return ResourcePressure::Unavailable;
    }
    let per_cpu = load[0] / f64::from(u32::try_from(cpus).unwrap_or(u32::MAX));
    if per_cpu >= CPU_CRITICAL_LOAD_PER_CPU {
        ResourcePressure::Critical
    } else if per_cpu >= CPU_ADVISORY_LOAD_PER_CPU {
        ResourcePressure::Advisory
    } else {
        ResourcePressure::Normal
    }
}

/// The worse of two readings, where NOT KNOWING is worse than being fine.
///
/// Operator ruling 01a04a38: "CPU is first-class — it can raise Critical too."
/// So neither input can cap the other, and taking the worse of the two is what
/// that means in code.
///
/// Unavailable ranks above Normal deliberately. If one signal is missing, the
/// machine has not been shown to be healthy — it has been shown to be healthy in
/// one respect and unmeasured in another, and this fleet has repeatedly shipped
/// the version where absence renders as good news.
const fn worst_pressure(left: ResourcePressure, right: ResourcePressure) -> ResourcePressure {
    match (left, right) {
        (ResourcePressure::Critical, _) | (_, ResourcePressure::Critical) => {
            ResourcePressure::Critical
        }
        (ResourcePressure::Advisory, _) | (_, ResourcePressure::Advisory) => {
            ResourcePressure::Advisory
        }
        (ResourcePressure::Unavailable, _) | (_, ResourcePressure::Unavailable) => {
            ResourcePressure::Unavailable
        }
        _ => ResourcePressure::Normal,
    }
}

fn sample_machine_resources() -> MachineResourceResponse {
    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
        let fields = meminfo.as_deref().map(parse_meminfo).unwrap_or_default();
        let bytes = |name: &str| fields.get(name).and_then(|value| value.checked_mul(1024));
        let memory_total_bytes = bytes("MemTotal");
        let memory_available_bytes = bytes("MemAvailable");
        let swap_total_bytes = bytes("SwapTotal");
        let swap_free_bytes = bytes("SwapFree");
        let swap_used_bytes = swap_total_bytes
            .zip(swap_free_bytes)
            .map(|(total, free)| total.saturating_sub(free));
        let percent = |used: u64, total: u64| {
            (total > 0).then(|| {
                let basis_points = used.saturating_mul(10_000) / total;
                f64::from(u32::try_from(basis_points).unwrap_or(u32::MAX)) / 100.0
            })
        };
        let memory_used_percent = memory_total_bytes
            .zip(memory_available_bytes)
            .and_then(|(total, available)| percent(total.saturating_sub(available), total));
        let swap_used_percent = swap_used_bytes
            .zip(swap_total_bytes)
            .and_then(|(used, total)| percent(used, total));
        let load_average = std::fs::read_to_string("/proc/loadavg")
            .ok()
            .and_then(|value| parse_load_average(&value));
        let memory_pressure_avg10 = read_psi_avg10("/proc/pressure/memory");
        let cpu_pressure_avg10 = read_psi_avg10("/proc/pressure/cpu");
        let io_pressure_avg10 = read_psi_avg10("/proc/pressure/io");
        // Memory, unchanged. These were tuned against a real report and 01a04982
        // fenced them explicitly.
        let memory = match (memory_used_percent, memory_pressure_avg10) {
            (_, Some(psi)) if psi >= 10.0 => ResourcePressure::Critical,
            (Some(used), _) if used >= 95.0 => ResourcePressure::Critical,
            (_, Some(psi)) if psi >= 2.0 => ResourcePressure::Advisory,
            (Some(used), _) if used >= 85.0 => ResourcePressure::Advisory,
            (Some(_), _) => ResourcePressure::Normal,
            _ => ResourcePressure::Unavailable,
        };
        // Read once and shared with the response below, so the verdict and the
        // number an operator reads in Diagnostics cannot disagree.
        let logical_cpus = std::thread::available_parallelism().ok().map(usize::from);
        let pressure = worst_pressure(memory, cpu_pressure(load_average, logical_cpus));
        MachineResourceResponse {
            memory_total_bytes,
            memory_available_bytes,
            memory_used_percent,
            swap_total_bytes,
            swap_used_bytes,
            swap_used_percent,
            load_average,
            logical_cpus,
            memory_pressure_avg10,
            cpu_pressure_avg10,
            io_pressure_avg10,
            pressure,
        }
    }
    #[cfg(not(target_os = "linux"))]
    MachineResourceResponse {
        memory_total_bytes: None,
        memory_available_bytes: None,
        memory_used_percent: None,
        swap_total_bytes: None,
        swap_used_bytes: None,
        swap_used_percent: None,
        load_average: None,
        logical_cpus: std::thread::available_parallelism().ok().map(usize::from),
        memory_pressure_avg10: None,
        cpu_pressure_avg10: None,
        io_pressure_avg10: None,
        pressure: ResourcePressure::Unavailable,
    }
}

pub(super) async fn coordinator_start_admission(state: &AppState) -> CoordinatorStartAdmission {
    let machine = sample_machine_resources();
    let terminal_host = if let Some(client) = &state.terminal_host {
        match client.request(&HostRequest::HostStatus).await {
            Ok(HostResponse::HostStatus { status }) => {
                coordinator_process_pressure(status.resources, &machine)
            }
            Ok(_) | Err(_) => ResourcePressure::Unavailable,
        }
    } else {
        ResourcePressure::Unavailable
    };
    combine_coordinator_start_admission(machine.pressure, terminal_host)
}

/// Whether the terminal host is the reason a machine is struggling.
///
/// Judged as a share of the machine, exactly as the diagnostics page judges it.
/// This was a fixed 4 GiB ceiling applied to the host's whole process tree —
/// which is every loaded worker together — so ten healthy workers on a large
/// machine crossed it and automatic starts were paused permanently. Queen then
/// queued wakes that were never claimed, and the operator watched workers she
/// said she had opened never come up.
///
/// The same mistake was fixed on the page that displays this; it survived here,
/// where it actually stops work.
pub(super) fn coordinator_process_pressure(
    sample: Option<ProcessResourceSample>,
    machine: &MachineResourceResponse,
) -> ResourcePressure {
    let bytes = sample.and_then(|sample| {
        sample
            .process_tree_resident_memory_bytes
            .or(sample.resident_memory_bytes)
    });
    layer_pressure(bytes, machine)
}

pub(super) const fn combine_coordinator_start_admission(
    machine: ResourcePressure,
    terminal_host: ResourcePressure,
) -> CoordinatorStartAdmission {
    if matches!(machine, ResourcePressure::Critical)
        || matches!(terminal_host, ResourcePressure::Critical)
    {
        CoordinatorStartAdmission::DeferredCritical
    } else if matches!(machine, ResourcePressure::Advisory)
        || matches!(terminal_host, ResourcePressure::Advisory)
    {
        CoordinatorStartAdmission::DeferredAdvisory
    } else if matches!(terminal_host, ResourcePressure::Unavailable) {
        // The worker engine must be reachable before an automatic start can be
        // safely claimed. Machine evidence is Linux-only, so a healthy engine
        // is sufficient on other supported hosts where that evidence is absent.
        CoordinatorStartAdmission::DeferredUnavailable
    } else {
        CoordinatorStartAdmission::Allowed
    }
}

#[cfg(target_os = "linux")]
fn parse_meminfo(value: &str) -> HashMap<&str, u64> {
    value
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let kib = rest.split_whitespace().next()?.parse().ok()?;
            Some((name, kib))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_load_average(value: &str) -> Option<[f64; 3]> {
    let mut values = value.split_whitespace().take(3).map(str::parse::<f64>);
    Some([
        values.next()?.ok()?,
        values.next()?.ok()?,
        values.next()?.ok()?,
    ])
}

#[cfg(target_os = "linux")]
fn read_psi_avg10(path: &str) -> Option<f64> {
    let value = std::fs::read_to_string(path).ok()?;
    value.lines().find_map(|line| {
        let rest = line.strip_prefix("some ")?;
        rest.split_whitespace()
            .find_map(|field| field.strip_prefix("avg10=")?.parse().ok())
    })
}

/// A layer is judged by how much of the machine it holds, and only when the
/// machine itself is under pressure. Below these shares the layer is not the
/// reason for a stall even when the machine is stalling.
const LAYER_ADVISORY_PERCENT: u64 = 15;
const LAYER_CRITICAL_PERCENT: u64 = 25;

/// How much of a machine one layer is holding, and whether that matters.
///
/// A byte count on its own says nothing. Six gigabytes across ten loaded
/// workers is unremarkable on a machine with thirty-two and fatal on one with
/// eight, and a fixed half-gigabyte ceiling reported ten healthy workers as
/// Critical while the kernel was reporting no memory stall at all.
///
/// So pressure is read from the machine — how much it has left, and whether it
/// is actually stalling — and a layer is named only when the machine is under
/// pressure and that layer is a large enough share to be worth looking at. The
/// question the page asks is which layer needs attention; when nothing does,
/// the answer is nothing.
fn layer_pressure(bytes: Option<u64>, machine: &MachineResourceResponse) -> ResourcePressure {
    let Some(bytes) = bytes else {
        return ResourcePressure::Unavailable;
    };
    // Integer percentage points rather than a ratio: the thresholds are whole
    // percentages, and a float here buys nothing but a lossy cast.
    let share = machine
        .memory_total_bytes
        .filter(|total| *total > 0)
        .map(|total| bytes.saturating_mul(100) / total);
    // Without the machine's size there is nothing to judge against, and a
    // guess dressed as a verdict is worse than saying so.
    let (Some(share), machine_pressure) = (share, machine.pressure) else {
        return ResourcePressure::Unavailable;
    };
    match machine_pressure {
        // A layer holding almost nothing is not the reason a machine is
        // struggling, whatever the machine is doing.
        ResourcePressure::Critical if share >= LAYER_CRITICAL_PERCENT => ResourcePressure::Critical,
        ResourcePressure::Critical | ResourcePressure::Advisory
            if share >= LAYER_ADVISORY_PERCENT =>
        {
            ResourcePressure::Advisory
        }
        _ => ResourcePressure::Normal,
    }
}

pub(super) fn resource_response(
    sample: Option<ProcessResourceSample>,
    machine: &MachineResourceResponse,
) -> ProcessResourceResponse {
    let resident_memory_bytes = sample.and_then(|sample| sample.resident_memory_bytes);
    let process_tree_resident_memory_bytes =
        sample.and_then(|sample| sample.process_tree_resident_memory_bytes);
    let process_tree_process_count = sample.and_then(|sample| sample.process_tree_process_count);
    let pressure = layer_pressure(
        process_tree_resident_memory_bytes.or(resident_memory_bytes),
        machine,
    );
    ProcessResourceResponse {
        resident_memory_bytes,
        process_tree_resident_memory_bytes,
        process_tree_process_count,
        pressure,
    }
}

/// What this build changes that a reload will NOT put into effect.
///
/// "It is live" means several different things here and which one applies
/// depends on what changed. Every instance of that has been found the same way:
/// after the code was written, by something not working, reading as a different
/// bug each time. A 422 naming a serde variant reads as a protocol bug; a
/// missing tool reads as an unbuilt feature.
///
/// So this states the fact rather than leaving it to be discovered. The API
/// already asks the terminal host for its status, so the difference between
/// what the host speaks and what this checkout speaks is computable now.
///
/// DOES NOT AND MUST NOT RESTART THE HOST. Deferring that update while sessions
/// are live is deliberate: it is what stops a reload killing every worker's
/// terminal mid-turn. This only reports.
pub(super) async fn worker_engine_update_required(state: &AppState) -> Option<String> {
    let client = state.terminal_host.as_ref()?;
    let running = match client.request(&HostRequest::HostStatus).await {
        Ok(HostResponse::HostStatus { status }) => status.protocol_version,
        // A host that cannot be reached is not a host that is out of date, and
        // guessing either way here would be worse than saying nothing.
        Ok(_) | Err(_) => return None,
    };
    (running != swarm_terminal::PROTOCOL_VERSION).then(|| {
        format!(
            "the worker engine update is required: this build speaks terminal protocol {}, \
             the running terminal host speaks {running}. A reload restarts the API and web \
             only -- the terminal host is a separate service so worker terminals survive it.",
            swarm_terminal::PROTOCOL_VERSION
        )
    })
}

#[cfg(test)]
mod tests {
    use super::reload_state_from;
    use super::{
        ResourcePressure, cpu_pressure, layer_pressure, machine_of as machine, worst_pressure,
    };

    /// Load-per-CPU, measured on this Hive rather than guessed.
    ///
    /// The idle band comes from 22 samples over two minutes on the operator's
    /// four-core box: load-per-CPU held 0.18-0.37, average 0.26. The two
    /// saturation events on record sat at 1.27 and 1.69 with a peak of 2.46.
    #[test]
    fn cpu_load_per_core_decides_and_the_idle_band_is_nowhere_near_it() {
        let four = Some(4);
        // Measured idle. This is the assertion the acceptance turns on: a
        // signal that fires on a quiet box is worse than no signal.
        assert_eq!(
            cpu_pressure(Some([1.04, 1.0, 1.0]), four),
            ResourcePressure::Normal
        );
        assert_eq!(
            cpu_pressure(Some([1.48, 1.4, 1.4]), four),
            ResourcePressure::Normal,
            "the busiest idle sample must still be Normal"
        );
        // Queen's observation, 5.07 on four cores.
        assert_eq!(
            cpu_pressure(Some([5.07, 4.9, 4.5]), four),
            ResourcePressure::Advisory
        );
        // The episode where builds ran fifteen times slow.
        assert_eq!(
            cpu_pressure(Some([6.76, 6.0, 5.5]), four),
            ResourcePressure::Advisory
        );
        // Its peak.
        assert_eq!(
            cpu_pressure(Some([9.82, 8.0, 6.0]), four),
            ResourcePressure::Critical
        );
    }

    /// The same load means different things on different machines.
    #[test]
    fn load_is_read_against_the_core_count_not_alone() {
        assert_eq!(
            cpu_pressure(Some([4.0, 4.0, 4.0]), Some(16)),
            ResourcePressure::Normal
        );
        assert_eq!(
            cpu_pressure(Some([4.0, 4.0, 4.0]), Some(1)),
            ResourcePressure::Critical
        );
    }

    /// Not knowing is not being fine.
    ///
    /// A load figure without a core count says nothing, and reporting Normal
    /// from it is the defect this fleet keeps shipping — absence rendering as
    /// good news.
    #[test]
    fn an_unreadable_cpu_signal_is_unknown_rather_than_healthy() {
        assert_eq!(cpu_pressure(None, Some(4)), ResourcePressure::Unavailable);
        assert_eq!(
            cpu_pressure(Some([1.0, 1.0, 1.0]), None),
            ResourcePressure::Unavailable
        );
        assert_eq!(
            cpu_pressure(Some([1.0, 1.0, 1.0]), Some(0)),
            ResourcePressure::Unavailable
        );
    }

    /// Operator ruling 01a04a38: "CPU is first-class — it can raise Critical
    /// too." Neither input may cap the other.
    ///
    /// ABLATION: make `worst_pressure` return its memory argument and the first
    /// assertion fails — that is the whole of the old behaviour.
    #[test]
    fn cpu_can_raise_critical_on_its_own_and_memory_is_not_capped_either() {
        // Memory perfectly clean, CPU desperate. This is the reported case.
        assert_eq!(
            worst_pressure(ResourcePressure::Normal, ResourcePressure::Critical),
            ResourcePressure::Critical
        );
        // And the reverse still holds: CPU idle does not soothe a dying memory.
        assert_eq!(
            worst_pressure(ResourcePressure::Critical, ResourcePressure::Normal),
            ResourcePressure::Critical
        );
        assert_eq!(
            worst_pressure(ResourcePressure::Normal, ResourcePressure::Advisory),
            ResourcePressure::Advisory
        );
        assert_eq!(
            worst_pressure(ResourcePressure::Normal, ResourcePressure::Normal),
            ResourcePressure::Normal
        );
    }

    /// One signal missing does not make the machine healthy, and does not hide
    /// the other signal shouting.
    #[test]
    fn an_unknown_half_never_reads_as_normal_and_never_masks_the_other() {
        assert_eq!(
            worst_pressure(ResourcePressure::Normal, ResourcePressure::Unavailable),
            ResourcePressure::Unavailable
        );
        assert_eq!(
            worst_pressure(ResourcePressure::Unavailable, ResourcePressure::Critical),
            ResourcePressure::Critical
        );
        assert_eq!(
            worst_pressure(ResourcePressure::Unavailable, ResourcePressure::Advisory),
            ResourcePressure::Advisory
        );
    }

    /// The operator, 2026-08-28: "I have a pending build showing that needs to
    /// be installed" — when the revision it was offering was already the one
    /// serving them the page.
    ///
    /// A reload was refused for a protocol change; the migration installed that
    /// very revision minutes later. The status file outlived the attempt, and
    /// the only staleness check compared it against the CHECKOUT, which still
    /// matched. Nothing compared it against what was running.
    #[test]
    fn a_failure_for_the_revision_now_running_is_not_reported() {
        let failed = "state=failed\nrevision=b9220d224bb2\n";
        assert_eq!(
            reload_state_from(failed, Some("b9220d224bb2"), Some("b9220d224bb2"), false),
            "idle"
        );
    }

    /// And a failure for a revision that is NOT deployed is still a failure —
    /// the case the card exists for.
    #[test]
    fn a_failure_for_a_revision_that_is_not_running_still_reports() {
        let failed = "state=failed\nrevision=b9220d224bb2\n";
        assert_eq!(
            reload_state_from(failed, Some("b9220d224bb2"), Some("5a966cfd048b"), false),
            "failed"
        );
    }

    /// A build in flight for the deployed revision is not a superseded failure.
    /// Only `failed` is forgotten; `building` still reports, or a reload of
    /// unchanged source would look idle while it ran.
    #[test]
    fn a_build_in_flight_is_untouched_by_the_supersede_rule() {
        let building = "state=building\nrevision=b9220d224bb2\n";
        assert_eq!(
            reload_state_from(building, Some("b9220d224bb2"), Some("b9220d224bb2"), false),
            "building"
        );
    }

    /// A Hive that cannot say what it was built from must not swallow failures:
    /// `None == None` would make every failure with no revision look superseded.
    #[test]
    fn an_unknown_deployed_revision_does_not_swallow_a_failure() {
        let failed = "state=failed\n";
        assert_eq!(reload_state_from(failed, None, None, false), "failed");
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    /// The report the operator sent: ten loaded worker runtimes holding six
    /// gigabytes, on a machine with thirty-two and no memory stall at all,
    /// were being called Critical against a fixed 512 MiB ceiling that a
    /// single provider process passes on its own.
    #[test]
    fn a_large_share_of_an_unstressed_machine_is_normal() {
        let machine = machine(32 * GIB, ResourcePressure::Normal);
        assert_eq!(
            layer_pressure(Some(6 * GIB), &machine),
            ResourcePressure::Normal
        );
    }

    /// The same six gigabytes on a machine a quarter the size, which is
    /// actually stalling, is the thing to look at.
    #[test]
    fn a_large_share_of_a_stalling_machine_is_critical() {
        let machine = machine(8 * GIB, ResourcePressure::Critical);
        assert_eq!(
            layer_pressure(Some(6 * GIB), &machine),
            ResourcePressure::Critical
        );
    }

    /// A layer holding almost nothing did not cause the stall and must not be
    /// what the page points at.
    #[test]
    fn a_small_share_of_a_stalling_machine_is_normal() {
        let machine = machine(32 * GIB, ResourcePressure::Critical);
        assert_eq!(
            layer_pressure(Some(GIB / 2), &machine),
            ResourcePressure::Normal
        );
    }

    /// Between the two: a meaningful share of a machine that is starting to
    /// stall is worth flagging, but is not yet the emergency.
    #[test]
    fn a_meaningful_share_of_a_pressured_machine_is_advisory() {
        let machine = machine(32 * GIB, ResourcePressure::Advisory);
        assert_eq!(
            layer_pressure(Some(6 * GIB), &machine),
            ResourcePressure::Advisory
        );
    }

    #[test]
    fn a_layer_without_a_measurement_reports_nothing_rather_than_healthy() {
        let healthy = machine(32 * GIB, ResourcePressure::Normal);
        let unmeasured = machine(0, ResourcePressure::Normal);
        assert_eq!(
            layer_pressure(None, &healthy),
            ResourcePressure::Unavailable
        );
        assert_eq!(
            layer_pressure(Some(GIB), &unmeasured),
            ResourcePressure::Unavailable
        );
    }
}
