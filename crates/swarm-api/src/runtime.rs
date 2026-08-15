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
    ApiError, AppState, MAX_TERMINAL_WEBSOCKETS, RESOURCE_ADVISORY_BYTES, RESOURCE_CRITICAL_BYTES,
    authorize, authorized_no_store_request, build_version, unix_timestamp,
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

#[derive(Debug, Serialize)]
struct MachineResourceResponse {
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

#[derive(Debug, Serialize)]
struct ResourcePolicyResponse {
    mode: &'static str,
    advisory_bytes: u64,
    critical_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResourcePressure {
    Normal,
    Advisory,
    Critical,
    Unavailable,
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
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(DevelopmentRuntimeResponse {
            enabled: state.development_reload_request_path.is_some(),
            version: build_version(),
            state: development_reload_state(&state),
            reload_available: source
                .as_ref()
                .is_some_and(|status| status.reload_available),
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
}

fn development_source_status(state: &AppState) -> Option<DevelopmentSourceStatus> {
    let checkout = state.development_checkout_path.as_ref()?;
    development_source_status_for(
        checkout,
        deployed_source_revision(build_version()).as_deref(),
    )
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
    let committed_changes = deployed_revision.is_none_or(|deployed| {
        Command::new("git")
            .arg("-C")
            .arg(checkout)
            .args(["diff", "--quiet", deployed, "HEAD", "--"])
            .args(DEVELOPMENT_PRODUCT_PATHS)
            .status()
            .map_or(true, |status| !status.success())
    });
    Some(DevelopmentSourceStatus {
        revision,
        dirty,
        reload_available: dirty || committed_changes,
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

pub(super) fn development_reload_state(state: &AppState) -> &'static str {
    let Some(path) = &state.development_reload_status_path else {
        return "disabled";
    };
    let Ok(value) = std::fs::read_to_string(path.as_ref()) else {
        return "idle";
    };
    match value.lines().find_map(|line| line.strip_prefix("state=")) {
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
    let terminal_host = if let Some(client) = &state.terminal_host {
        match client.request(&HostRequest::HostStatus).await {
            Ok(HostResponse::HostStatus { status }) => resource_response(status.resources),
            Ok(_) | Err(_) => resource_response(None),
        }
    } else {
        resource_response(None)
    };
    let response = RuntimeResourcesResponse {
        sampled_at: unix_timestamp(),
        policy: ResourcePolicyResponse {
            mode: "observe_only",
            advisory_bytes: RESOURCE_ADVISORY_BYTES,
            critical_bytes: RESOURCE_CRITICAL_BYTES,
        },
        api: resource_response(Some(sample_current_process())),
        terminal_host,
        machine: sample_machine_resources(),
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

pub(super) fn resource_response(sample: Option<ProcessResourceSample>) -> ProcessResourceResponse {
    let resident_memory_bytes = sample.and_then(|sample| sample.resident_memory_bytes);
    let process_tree_resident_memory_bytes =
        sample.and_then(|sample| sample.process_tree_resident_memory_bytes);
    let process_tree_process_count = sample.and_then(|sample| sample.process_tree_process_count);
    let pressure = match process_tree_resident_memory_bytes.or(resident_memory_bytes) {
        Some(bytes) if bytes >= RESOURCE_CRITICAL_BYTES => ResourcePressure::Critical,
        Some(bytes) if bytes >= RESOURCE_ADVISORY_BYTES => ResourcePressure::Advisory,
        Some(_) => ResourcePressure::Normal,
        None => ResourcePressure::Unavailable,
    };
    ProcessResourceResponse {
        resident_memory_bytes,
        process_tree_resident_memory_bytes,
        process_tree_process_count,
        pressure,
    }
}
