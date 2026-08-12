import { useEffect, useState } from "react";

import {
  fetchHistoryDiagnostics,
  fetchRuntimeResources,
  fetchTerminalHostStatus,
  type ControlRoomEvent,
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
  operatorToken: string;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  workers: Worker[];
};

export default function DiagnosticsWorkspace({ operatorToken, health, hiveIdentity, liveFeedState, recentEvents, sessions, workers }: Props) {
  const [runtime, setRuntime] = useState<RuntimeDiagnostics>({ loaded: false });
  const [preview, setPreview] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "unavailable">("idle");

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

  return (
    <section className="settings-card diagnostics-card" aria-labelledby="diagnostics-heading">
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
