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
}

#[derive(Debug, Serialize)]
struct DevelopmentRuntimeResponse {
    enabled: bool,
    version: &'static str,
    state: &'static str,
    reload_available: bool,
    deployed_source_revision: Option<String>,
    source_revision: Option<String>,
    source_dirty: bool,
}

#[derive(Debug, Serialize)]
struct RuntimeResourcesResponse {
    sampled_at: i64,
    policy: ResourcePolicyResponse,
    api: ProcessResourceResponse,
    terminal_host: ProcessResourceResponse,
    machine: MachineResourceResponse,
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
            source_dirty: source.is_some_and(|status| status.dirty),
        }),
    )
        .into_response())
}

pub(super) struct DevelopmentSourceStatus {
    pub(super) revision: String,
    pub(super) dirty: bool,
    pub(super) reload_available: bool,
    pub(super) aligned: bool,
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
    Some(DevelopmentSourceStatus {
        revision,
        dirty,
        reload_available: aligned && (dirty || committed_changes),
        aligned,
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

pub(super) fn development_reload_state_for_source(
    state: &AppState,
    source_revision: Option<&str>,
) -> &'static str {
    let Some(path) = &state.development_reload_status_path else {
        return "disabled";
    };
    let Ok(value) = std::fs::read_to_string(path.as_ref()) else {
        return "idle";
    };
    let marker_revision = value
        .lines()
        .find_map(|line| line.strip_prefix("revision="));
    let marker_state = value.lines().find_map(|line| line.strip_prefix("state="));
    if matches!(marker_state, Some("requested" | "building" | "failed"))
        && marker_revision != source_revision
    {
        return "idle";
    }
    match marker_state {
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
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
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
        let pressure = match (memory_used_percent, memory_pressure_avg10) {
            (_, Some(psi)) if psi >= 10.0 => ResourcePressure::Critical,
            (Some(used), _) if used >= 95.0 => ResourcePressure::Critical,
            (_, Some(psi)) if psi >= 2.0 => ResourcePressure::Advisory,
            (Some(used), _) if used >= 85.0 => ResourcePressure::Advisory,
            (Some(_), _) => ResourcePressure::Normal,
            _ => ResourcePressure::Unavailable,
        };
        MachineResourceResponse {
            memory_total_bytes,
            memory_available_bytes,
            memory_used_percent,
            swap_total_bytes,
            swap_used_bytes,
            swap_used_percent,
            load_average,
            logical_cpus: std::thread::available_parallelism().ok().map(usize::from),
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

#[cfg(test)]
mod tests {
    use super::{ResourcePressure, layer_pressure, machine_of as machine};

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
