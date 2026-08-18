import { useEffect, useState } from "react";

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
  type RuntimeResources,
  type SessionSummary,
  type TerminalHostStatus,
  type Worker,
} from "../api";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import { serializeDiagnosticReport, type RuntimeDiagnostics } from "./diagnosticReport";

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
  const [runtimeRevision, setRuntimeRevision] = useState(0);
  const [preview, setPreview] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "unavailable">("idle");
  const [savedReports, setSavedReports] = useState<DogfoodReport[]>();
  const [savedReportsUnavailable, setSavedReportsUnavailable] = useState(false);
  const [copiedReportId, setCopiedReportId] = useState<string>();
  const [downloadingReportId, setDownloadingReportId] = useState<string>();
  const [unavailableAttachmentId, setUnavailableAttachmentId] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    async function refreshRuntime() {
      const [host, history, resources] = await Promise.allSettled([
        fetchTerminalHostStatus(operatorToken),
        fetchHistoryDiagnostics(operatorToken),
        fetchRuntimeResources(operatorToken),
      ]);
      if (cancelled) return;
      setRuntime({
        terminalHost: host.status === "fulfilled" ? host.value : undefined,
        history: history.status === "fulfilled" ? history.value : undefined,
        resources: resources.status === "fulfilled" ? resources.value : undefined,
        loaded: true,
      });
    }
    void refreshRuntime();
    const timer = window.setInterval(() => void refreshRuntime(), 10_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [operatorToken, runtimeRevision]);

  useEffect(() => {
    let cancelled = false;
    void fetchDogfoodReports(operatorToken)
      .then((reports) => { if (!cancelled) setSavedReports(reports); })
      .catch(() => { if (!cancelled) setSavedReportsUnavailable(true); });
    return () => { cancelled = true; };
  }, [operatorToken, feedbackRevision]);

  const launchFailures = workers.filter((worker) => Boolean(worker.runtime_error)).length;
  const providerStatus = launchFailures > 0 ? "Needs attention" : "Healthy";
  const terminalStatus = !runtime.loaded ? "Checking…" : runtime.terminalHost
    ? runtime.terminalHost.draining ? "Updating safely" : `Healthy · ${runtime.terminalHost.host_version}`
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

  return (
    <section id="settings-diagnostics" className="settings-card diagnostics-card" aria-labelledby="diagnostics-heading">
      <div><p className="eyebrow">Diagnostics</p><h3 id="diagnostics-heading">Know which layer needs attention</h3></div>
      <p>The report is previewed before copying and never includes terminal text, task content, workspace paths, credentials, or raw errors.</p>
      <div className="diagnostic-live-status" role="status">
        <span><span className={`presence ${runtime.loaded ? "online" : ""}`} /><span><strong>{runtime.loaded ? "Live metrics" : "Checking metrics"}</strong><small>{runtime.resources ? `Sampled ${formatSampleTime(runtime.resources.sampled_at)} · refreshes every 10 seconds` : "Waiting for the runtime and terminal host"}</small></span></span>
        <button type="button" className="secondary-button" onClick={() => setRuntimeRevision((current) => current + 1)}>Refresh now</button>
      </div>
      <dl className="diagnostic-list">
        <div><dt>Browser</dt><dd>{navigator.onLine ? "Online" : "Offline"}</dd></div>
        <div><dt>API</dt><dd>{health ? `Healthy · ${health.version}` : "Unavailable"}</dd></div>
        <div><dt>Database</dt><dd>{hiveIdentity ? "Healthy" : "Unavailable"}</dd></div>
        <div><dt>Terminal host</dt><dd>{terminalStatus}</dd></div>
        <div><dt>API memory</dt><dd className={resourceClass(runtime.resources?.api.pressure)}>{apiMemory}</dd></div>
        <div><dt>Terminal host service</dt><dd>{hostMemory}</dd></div>
        <div><dt>Loaded worker runtimes</dt><dd className={resourceClass(workerPressure)}>{workerMemory}</dd></div>
        <div><dt>Machine memory</dt><dd className={resourceClass(machine?.pressure)}>{machineMemoryLabel(machine)}</dd></div>
        <div><dt>Memory stall</dt><dd className={resourceClass(machine?.pressure)}>{pressureLabel(machine?.memory_pressure_avg10)}</dd></div>
        <div><dt>Compute load</dt><dd>{loadLabel(machine?.load_average, machine?.logical_cpus)}</dd></div>
        <div><dt>Standing swap</dt><dd>{swapLabel(machine?.swap_used_bytes, machine?.swap_total_bytes, machine?.swap_used_percent)}</dd></div>
        <div><dt>Provider</dt><dd>{providerStatus}</dd></div>
        <div><dt>Jira</dt><dd>{jiraStatusLabel(jiraReadiness, jiraUnavailable)}</dd></div>
      </dl>
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
        {savedReportsUnavailable ? <p role="status">Saved reports are unavailable right now.</p> : savedReports === undefined ? <p>Loading saved reports…</p> : savedReports.length === 0 ? <p>No reports saved yet.</p> : (
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
function workerTreePressure(resource: RuntimeResources["api"] | undefined): RuntimeResources["api"]["pressure"] | undefined {
  if (resource?.process_tree_resident_memory_bytes == null) return undefined;
  const workerBytes = Math.max(0, resource.process_tree_resident_memory_bytes - (resource.resident_memory_bytes ?? 0));
  if (workerBytes >= 4 * 1024 * 1024 * 1024) return "critical";
  if (workerBytes >= 2 * 1024 * 1024 * 1024) return "advisory";
  return "normal";
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
