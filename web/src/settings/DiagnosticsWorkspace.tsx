import { useCallback, useState } from "react";
import { useVisiblePolling } from "../runtime/useVisiblePolling";
import { readRoutePaints, routePaintSummary } from "../runtime/routePaint";
import { readBrowserPerformance } from "../runtime/browserPerformance";

import {
  downloadDogfoodScreenshot,
  fetchHistoryDiagnostics,
  fetchDogfoodReports,
  fetchRuntimeResources,
  fetchTerminalHostStatus,
  type ControlRoomEvent,
  type DogfoodReport,
  type Health,
  type HistoryDiagnostics,
  type HiveIdentity,
  type JiraReadiness,
  type MachineResources,
  type ResourcePressure,
  type RuntimeResources,
  type SessionSummary,
  type TerminalHostStatus,
  type Worker,
} from "../api";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import { serializeDiagnosticReport, type RuntimeDiagnostics } from "./diagnosticReport";
import { runtimeVersionIdentity } from "./runtimeVersion";
import { assessPerformance, computePressure } from "./performanceAssessment";
import PerformanceEvidence from "./PerformanceEvidence";

type Props = {
  feedbackRevision: number;
  operatorToken: string;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  workers: Worker[];
  jiraReadiness: JiraReadiness | undefined;
  jiraUnavailable: boolean;
};

export default function DiagnosticsWorkspace({ feedbackRevision, operatorToken, health, hiveIdentity, liveFeedState, recentEvents, sessions, workers, jiraReadiness, jiraUnavailable }: Props) {
  const [runtime, setRuntime] = useState<RuntimeDiagnostics>({ loaded: false });
  const [preview, setPreview] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "unavailable">("idle");
  const [savedReports, setSavedReports] = useState<DogfoodReport[]>();
  const [savedReportsUnavailable, setSavedReportsUnavailable] = useState(false);
  const [copiedReportId, setCopiedReportId] = useState<string>();
  const [downloadingReportId, setDownloadingReportId] = useState<string>();
  const [showEveryCheck, setShowEveryCheck] = useState(false);
  const [unavailableAttachmentId, setUnavailableAttachmentId] = useState<string>();

  const loadRuntime = useCallback(async (signal: AbortSignal) => {
    const [host, history, resources] = await Promise.allSettled([
      fetchTerminalHostStatus(operatorToken, signal),
      fetchHistoryDiagnostics(operatorToken, signal),
      fetchRuntimeResources(operatorToken, signal),
    ]);
    if (signal.aborted && !(signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) return;
    setRuntime({
      terminalHost: host.status === "fulfilled" ? host.value : undefined,
      history: history.status === "fulfilled" ? history.value : undefined,
      resources: resources.status === "fulfilled" ? resources.value : undefined,
      loaded: true,
    });
  }, [operatorToken]);
  const refreshRuntime = useVisiblePolling(loadRuntime, Boolean(operatorToken), 10_000);

  // A new feedback revision replaces any in-flight read of the previous list.
  const loadSavedReports = useCallback(async (signal: AbortSignal) => {
    try {
      const reports = await fetchDogfoodReports(operatorToken, 5, signal);
      if (signal.aborted) return;
      setSavedReports(reports);
      setSavedReportsUnavailable(false);
    } catch {
      if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) {
        setSavedReportsUnavailable(true);
      }
    }
  }, [operatorToken, feedbackRevision]);
  const refreshSavedReports = useVisiblePolling(loadSavedReports, Boolean(operatorToken), null);

  const launchFailures = workers.filter((worker) => Boolean(worker.runtime_error)).length;
  const providerStatus = launchFailures > 0 ? "Needs attention" : "Healthy";
  const routePaint = routePaintSummary(readRoutePaints());
  const browserTiming = readBrowserPerformance();
  const terminalStatus = !runtime.loaded ? "Checking…" : runtime.terminalHost
    ? runtime.terminalHost.draining ? "Updating safely" : `Healthy · ${runtimeVersionIdentity(runtime.terminalHost.host_version)}`
    : "Unavailable";
  const apiMemory = resourceLabel(runtime.resources?.api);
  const hostMemory = ownResourceLabel(runtime.resources?.terminal_host);
  const workerMemory = workerTreeLabel(runtime.resources?.terminal_host, runtime.terminalHost?.running_sessions ?? sessions.filter((session) => session.running).length);
  const workerPressure = workerTreePressure(runtime.resources?.terminal_host);
  const machine = runtime.resources?.machine;
  const measuredWorkers = workers
    .filter((worker) => worker.running && worker.active_session_id)
    .map((worker) => ({ worker, session: sessions.find((session) => session.session_id === worker.active_session_id) }))
    .filter(({ session }) => session?.resources?.process_tree_resident_memory_bytes != null);

  function buildPreview() {
    const serialized = serializeDiagnosticReport({ health, hiveIdentity, liveFeedState, recentEvents, runtime, sessions, workers, jiraReadiness, jiraUnavailable });
    setPreview(serialized);
    setCopyState("idle");
    return serialized;
  }

  async function copyReport() {
    const report = preview ?? buildPreview();
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(report);
      setCopyState("copied");
    } catch {
      setCopyState("unavailable");
    }
  }

  async function copySavedReport(report: DogfoodReport) {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(report.diagnostic_bundle);
      setCopiedReportId(report.id);
    } catch {
      setCopiedReportId(undefined);
    }
  }

  async function downloadScreenshot(report: DogfoodReport) {
    if (!report.attachment_name) return;
    setDownloadingReportId(report.id);
    setUnavailableAttachmentId(undefined);
    try {
      const blob = await downloadDogfoodScreenshot(operatorToken, report.attachment_name);
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = report.attachment_name;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch {
      setUnavailableAttachmentId(report.id);
    } finally {
      setDownloadingReportId(undefined);
    }
  }

  /**
   * Every check, each carrying its own verdict.
   *
   * The verdict is what makes the page scan: it decides what leads and what
   * collapses. A row with no way to be wrong is healthy by construction rather
   * than by omission, so nothing disappears from the full list.
   */
  const rows: { label: string; value: string; healthy: boolean; className?: string }[] = [
    { label: "Browser", value: navigator.onLine ? "Online" : "Offline", healthy: navigator.onLine },
    {
      label: "View switching",
      value: routePaint
        ? `${routePaint.median_ms} ms typical · ${routePaint.slowest_ms} ms slowest of ${routePaint.samples}`
        : "No view changes measured yet",
      healthy: true,
    },
    { label: "API", value: health ? `Healthy · ${health.version}` : "Unavailable", healthy: Boolean(health) },
    { label: "Database", value: hiveIdentity ? "Reachable · integrity not checked" : "Unavailable", healthy: Boolean(hiveIdentity) },
    { label: "Terminal host", value: terminalStatus, healthy: terminalStatus !== "Unavailable" },
    { label: "API memory", value: apiMemory, healthy: healthyPressure(runtime.resources?.api.pressure), className: resourceClass(runtime.resources?.api.pressure) },
    { label: "Terminal host service", value: hostMemory, healthy: true },
    { label: "Loaded worker runtimes", value: workerMemory, healthy: healthyPressure(workerPressure), className: resourceClass(workerPressure) },
    { label: "Machine memory", value: machineMemoryLabel(machine), healthy: healthyPressure(machine?.pressure), className: resourceClass(machine?.pressure) },
    { label: "Memory stall", value: pressureLabel(machine?.memory_pressure_avg10), healthy: healthyPressure(machine?.pressure), className: resourceClass(machine?.pressure) },
    { label: "Compute load", value: loadLabel(machine?.load_average, machine?.logical_cpus), healthy: healthyPressure(computePressure(machine)), className: resourceClass(computePressure(machine)) },
    { label: "Standing swap", value: swapLabel(machine?.swap_used_bytes, machine?.swap_total_bytes, machine?.swap_used_percent), healthy: true },
    { label: "Provider", value: providerStatus, healthy: launchFailures === 0 },
    { label: "Jira", value: jiraStatusLabel(jiraReadiness, jiraUnavailable), healthy: !jiraStatusLabel(jiraReadiness, jiraUnavailable).includes("attention") },
    // A SUBSYSTEM THAT STEPPED ASIDE HAS TO SAY SO SOMEWHERE SOMEONE LOOKS.
    // These used to abort startup, so their failure was impossible to miss and
    // impossible to recover from. Now the Hive keeps serving — which means a
    // misconfiguration can sit unnoticed for days unless it is reported, and
    // "email quietly never sent" is its own kind of outage. Carried as rows
    // rather than a banner so they land in needsAttention with everything
    // else, and lead the page for the same reason every other fault does.
    ...(health?.degraded ?? []).map((entry) => ({
      label: `${entry.subsystem} configuration`,
      value: `Disabled at startup · ${entry.reason}`,
      healthy: false,
    })),
  ];
  const needsAttention = rows.filter((row) => !row.healthy);

  return (
    <section id="settings-diagnostics" className="settings-card diagnostics-card" aria-labelledby="diagnostics-heading">
      <div><p className="eyebrow">Diagnostics</p><h3 id="diagnostics-heading">Know which layer needs attention</h3></div>
      <p>The report is previewed before copying and never includes terminal text, task content, workspace paths, credentials, or raw errors.</p>
      <div className="diagnostic-live-status" role="status">
        <span><span className={`presence ${runtime.loaded ? "online" : ""}`} /><span><strong>{runtime.loaded ? "Live metrics" : "Checking metrics"}</strong><small>{runtime.resources ? `Sampled ${formatSampleTime(runtime.resources.sampled_at)} · refreshes every 10 seconds` : "Waiting for the runtime and terminal host"}</small></span></span>
        <button type="button" className="secondary-button" onClick={() => void refreshRuntime()}>Refresh now</button>
      </div>
      {/* What everything below is relative to. Six gigabytes of workers means
          something different on a machine with thirty-two than on one with
          eight, and every row underneath was being read without that. */}
      <p className={`diagnostic-machine ${resourceClass(machine?.pressure)}`} role="status">
        {machineHeadline(machine)}
      </p>
      {/* Fourteen rows of equal weight under a heading promising to say which
          layer needs attention did not answer it. What is wrong leads; what is
          fine collapses. When nothing is wrong the honest page is one line. */}
      <PerformanceEvidence evidence={assessPerformance(browserTiming, runtime.resources)} />
      {needsAttention.length === 0 ? (
        <p className="diagnostic-verdict healthy" role="status">No faults reported by the available checks.</p>
      ) : (
        <p className="diagnostic-verdict attention" role="status">
          {needsAttention.length === 1 ? "One check needs attention: " : `${needsAttention.length} checks need attention: `}
          {needsAttention.map((row) => row.label).join(", ")}.
        </p>
      )}
      <dl className="diagnostic-list">
        {(showEveryCheck ? rows : needsAttention).map((row) => (
          <div key={row.label}><dt>{row.label}</dt><dd className={row.className}>{row.value}</dd></div>
        ))}
      </dl>
      <button type="button" className="diagnostic-show-all" aria-expanded={showEveryCheck} onClick={() => setShowEveryCheck((current) => !current)}>
        {showEveryCheck ? "Show only what needs attention" : `Show all ${rows.length} checks`}
      </button>
      <details className="browser-performance-breakdown">
        <summary>Browser performance evidence</summary>
        <p>{browserTiming.collection === "active" ? "Local timing capture is active." : "Local timing capture is not installed in this view."} Content-free evidence is retained for up to one hour; incident snapshots expire after 24 hours.</p>
        <p>{browserTiming.supported_observers.length ? `Native observers: ${browserTiming.supported_observers.join(", ")}.` : "Native long-task and interaction observers are unavailable; application timings may still be recorded."}</p>
        <p>{browserTiming.current.buckets.length} timing buckets · {browserTiming.current.incidents.length} recent incident captures. These are historical evidence, not unresolved alerts.</p>
        {browserTiming.before_reload ? <p>Before-reload snapshot available for comparison.</p> : null}
        <p>Preview report includes the timing evidence. Browser CPU percentage is not available here; compare with your browser task manager.</p>
      </details>
      {measuredWorkers.length ? (
        <details className="worker-resource-breakdown">
          <summary>Memory by loaded worker</summary>
          <dl className="diagnostic-list">
            {measuredWorkers.map(({ worker, session }) => <div key={worker.id}><dt>{worker.name}</dt><dd>{formatBytes(session!.resources!.process_tree_resident_memory_bytes!)} · {session!.resources!.process_tree_process_count ?? 1} processes</dd></div>)}
          </dl>
        </details>
      ) : null}
      <div className="diagnostic-actions">
        <button type="button" onClick={buildPreview}>Preview report</button>
        <button type="button" onClick={() => void copyReport()}>{copyState === "copied" ? "Copied" : "Copy report"}</button>
      </div>
      {copyState === "unavailable" ? <p role="status">Clipboard access is unavailable. Select the preview and copy it manually.</p> : null}
      {preview ? <pre className="diagnostic-preview" aria-label="Sanitized diagnostic report">{preview}</pre> : null}
      <div className="saved-feedback" aria-labelledby="saved-feedback-heading">
        <div><h4 id="saved-feedback-heading">Saved dogfood reports</h4><small>Private to this Hive · newest first</small></div>
        {savedReportsUnavailable ? <div className="saved-feedback-error" role="status"><span>Saved reports are unavailable right now.</span><button type="button" className="secondary-button" onClick={() => void refreshSavedReports()}>Retry saved reports</button></div> : savedReports === undefined ? <p>Loading saved reports…</p> : savedReports.length === 0 ? <p>No reports saved yet.</p> : (
          <div className="saved-feedback-list">
            {savedReports.map((report) => (
              <details key={report.id}>
                <summary><span>{report.observation.trim() || report.expectation.trim()}</span><time dateTime={new Date(report.created_at * 1000).toISOString()}>{formatReportDate(report.created_at)}</time></summary>
                <dl>
                  <div><dt>Expected</dt><dd>{report.expectation || "Not provided"}</dd></div>
                  <div><dt>Observed</dt><dd>{report.observation || "Not provided"}</dd></div>
                  <div><dt>Screenshot</dt><dd>{report.attachment_name ? "Attached privately" : "None"}</dd></div>
                </dl>
                <div className="saved-feedback-actions">
                  <button type="button" className="secondary-button" onClick={() => void copySavedReport(report)}>{copiedReportId === report.id ? "Copied report" : "Copy report for developer"}</button>
                  {report.attachment_name ? <button type="button" className="secondary-button" disabled={downloadingReportId === report.id} onClick={() => void downloadScreenshot(report)}>{downloadingReportId === report.id ? "Downloading…" : "Download screenshot"}</button> : null}
                </div>
                {unavailableAttachmentId === report.id ? <p role="status" className="saved-feedback-error">Screenshot is no longer available.</p> : null}
              </details>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
export function resourceLabel(resource: RuntimeResources["api"] | undefined) {
  if (!resource || resource.resident_memory_bytes === null) return "Unavailable";
  const memory = formatBytes(resource.resident_memory_bytes);
  if (resource.pressure === "critical") return `Critical · ${memory}`;
  if (resource.pressure === "advisory") return `Watch · ${memory}`;
  return `Normal · ${memory}`;
}

function resourceClass(pressure: RuntimeResources["api"]["pressure"] | undefined) {
  if (pressure === "critical") return "resource-pressure critical";
  if (pressure === "advisory") return "resource-pressure advisory";
  if (pressure === "normal") return "resource-pressure normal";
  return "resource-pressure unavailable";
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KiB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
}

function ownResourceLabel(resource: RuntimeResources["api"] | undefined) {
  return resource?.resident_memory_bytes == null ? "Unavailable" : formatBytes(resource.resident_memory_bytes);
}

function workerTreeLabel(resource: RuntimeResources["api"] | undefined, sessions: number) {
  if (resource?.process_tree_resident_memory_bytes == null) return "Unavailable";
  const workerBytes = Math.max(0, resource.process_tree_resident_memory_bytes - (resource.resident_memory_bytes ?? 0));
  const pressure = workerTreePressure(resource);
  const label = pressure === "critical" ? "Critical" : pressure === "advisory" ? "Watch" : "Normal";
  return `${label} · ${formatBytes(workerBytes)} · ${sessions} loaded`;
}

// Provider runtimes are expected to be much larger than the Rust service.
// Match the 2/4 GiB automatic-start admission bands instead of applying the
// service's 256/512 MiB limits to Claude or Codex process trees.
/**
 * The verdict on the loaded worker runtimes, taken from the server rather than
 * recomputed here.
 *
 * This used to carry its own thresholds — 4 GiB critical, 2 GiB advisory — and
 * they ignored the machine entirely. On a 31 GiB machine reporting no memory
 * stall at all, fifteen healthy workers holding 7 GiB were reported Critical.
 * That is the same defect already fixed in `layer_pressure`, living a second
 * time in a copy nobody updated: a byte count on its own says nothing, and six
 * gigabytes is unremarkable on thirty-two and fatal on eight.
 *
 * The terminal host's process tree is the loaded worker runtimes plus the host
 * itself, so its pressure is the verdict for this row. One implementation, on
 * the side that can see the machine.
 */
export function workerTreePressure(resource: RuntimeResources["api"] | undefined): RuntimeResources["api"]["pressure"] | undefined {
  if (resource?.process_tree_resident_memory_bytes == null) return undefined;
  return resource.pressure;
}

function machineMemoryLabel(machine: RuntimeResources["machine"] | undefined) {
  if (machine?.memory_total_bytes == null || machine.memory_available_bytes == null || machine.memory_used_percent == null) return "Unavailable";
  const used = machine.memory_total_bytes - machine.memory_available_bytes;
  return `${formatBytes(used)} / ${formatBytes(machine.memory_total_bytes)} · ${machine.memory_used_percent.toFixed(0)}% used`;
}

function pressureLabel(value: number | null | undefined) {
  return value == null ? "Unavailable" : `${value.toFixed(1)}% waiting · last 10 seconds`;
}

function loadLabel(load: [number, number, number] | null | undefined, cpus: number | null | undefined) {
  return !load ? "Unavailable" : `${load.map((value) => value.toFixed(2)).join(" / ")} · ${cpus ?? "?"} CPUs`;
}

function swapLabel(used: number | null | undefined, total: number | null | undefined, percent: number | null | undefined) {
  if (used == null || total == null || percent == null) return "Unavailable";
  return `${formatBytes(used)} / ${formatBytes(total)} · ${percent.toFixed(0)}% parked`;
}

function formatReportDate(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp * 1000));
}

function formatSampleTime(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { timeStyle: "medium" }).format(new Date(timestamp * 1000));
}

export function jiraStatusLabel(readiness: JiraReadiness | undefined, unavailable: boolean) {
  if (unavailable) return "Unavailable";
  if (!readiness) return "Checking…";
  if (!readiness.configured) return "Not connected";
  if (readiness.connection === "ready") return readiness.account_name ? `Connected · ${readiness.account_name}` : "Connected";
  if (readiness.connection === "network_unavailable") return "Network unavailable";
  if (readiness.connection === "credentials_invalid") return "Credentials need attention";
  if (readiness.connection === "permission_denied") return "Permission needs attention";
  return "Not connected";
}

/**
 * What this machine is, and how it is coping.
 *
 * Stated once at the top because every memory figure below is only meaningful
 * against it. The verdict comes from the kernel's own pressure reporting rather
 * than from a byte count, which is what made ten healthy workers read as
 * Critical on a machine that was not stalling at all.
 */
function machineHeadline(machine: MachineResources | undefined): string {
  if (!machine || machine.memory_total_bytes == null) return "Machine capacity unavailable";
  const cpus = machine.logical_cpus ? `${machine.logical_cpus} CPU${machine.logical_cpus === 1 ? "" : "s"}` : "unknown CPUs";
  const verdict = machine.pressure === "critical"
    ? "under memory pressure"
    : machine.pressure === "advisory"
      ? "starting to feel memory pressure"
      : machine.pressure === "normal" ? "not under memory pressure" : "memory pressure unavailable";
  return `${formatBytes(machine.memory_total_bytes)} of memory · ${cpus} · ${verdict}`;
}

/**
 * Whether the machine's processors are the thing to look at.
 *
 * Prefers the kernel's stall reporting; falls back to load against the number
 * of processors, because a load of four means idle on forty cores and saturated
 * on four.
 */
/** Unavailable is not unhealthy: it is a measurement that could not be taken. */
function healthyPressure(pressure: ResourcePressure | undefined): boolean {
  return pressure !== "advisory" && pressure !== "critical";
}
