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
};

export default function DiagnosticsWorkspace({ feedbackRevision, operatorToken, health, hiveIdentity, liveFeedState, recentEvents, sessions, workers }: Props) {
  const [runtime, setRuntime] = useState<RuntimeDiagnostics>({ loaded: false });
  const [preview, setPreview] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "unavailable">("idle");
  const [savedReports, setSavedReports] = useState<DogfoodReport[]>();
  const [savedReportsUnavailable, setSavedReportsUnavailable] = useState(false);
  const [copiedReportId, setCopiedReportId] = useState<string>();
  const [downloadingReportId, setDownloadingReportId] = useState<string>();
  const [unavailableAttachmentId, setUnavailableAttachmentId] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([
      fetchTerminalHostStatus(operatorToken),
      fetchHistoryDiagnostics(operatorToken),
      fetchRuntimeResources(operatorToken),
    ]).then(([host, history, resources]) => {
      if (cancelled) return;
      setRuntime({
        terminalHost: host.status === "fulfilled" ? host.value : undefined,
        history: history.status === "fulfilled" ? history.value : undefined,
        resources: resources.status === "fulfilled" ? resources.value : undefined,
        loaded: true,
      });
    });
    return () => { cancelled = true; };
  }, [operatorToken]);

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
  const hostMemory = resourceLabel(runtime.resources?.terminal_host);

  function buildPreview() {
    const serialized = serializeDiagnosticReport({ health, hiveIdentity, liveFeedState, recentEvents, runtime, sessions, workers });
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
      <dl className="diagnostic-list">
        <div><dt>Browser</dt><dd>{navigator.onLine ? "Online" : "Offline"}</dd></div>
        <div><dt>API</dt><dd>{health ? `Healthy · ${health.version}` : "Unavailable"}</dd></div>
        <div><dt>Database</dt><dd>{hiveIdentity ? "Healthy" : "Unavailable"}</dd></div>
        <div><dt>Terminal host</dt><dd>{terminalStatus}</dd></div>
        <div><dt>API memory</dt><dd className={resourceClass(runtime.resources?.api.pressure)}>{apiMemory}</dd></div>
        <div><dt>Terminal memory</dt><dd className={resourceClass(runtime.resources?.terminal_host.pressure)}>{hostMemory}</dd></div>
        <div><dt>Provider</dt><dd>{providerStatus}</dd></div>
        <div><dt>Integrations</dt><dd>Not configured</dd></div>
      </dl>
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
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

function formatReportDate(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp * 1000));
}
