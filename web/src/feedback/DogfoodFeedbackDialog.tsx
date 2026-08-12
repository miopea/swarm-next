import { useEffect, useState } from "react";

import {
  fetchHistoryDiagnostics,
  fetchRuntimeResources,
  fetchTerminalHostStatus,
  type ControlRoomEvent,
  type Health,
  type HiveIdentity,
  type SessionSummary,
  type Worker,
} from "../api";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import { serializeDiagnosticReport, type RuntimeDiagnostics } from "../settings/diagnosticReport";

type Props = {
  activeSessionId: string | undefined;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  onClose: () => void;
  operatorToken: string;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  surface: string;
  workers: Worker[];
};

export default function DogfoodFeedbackDialog({ activeSessionId, health, hiveIdentity, liveFeedState, onClose, operatorToken, recentEvents, sessions, surface, workers }: Props) {
  const [expectation, setExpectation] = useState("");
  const [observation, setObservation] = useState("");
  const [runtime, setRuntime] = useState<RuntimeDiagnostics>({ loaded: false });
  const [preview, setPreview] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "unavailable">("idle");

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

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

  function buildPreview() {
    const report = serializeDiagnosticReport({
      context: { expectation, observation, selectedSessionId: activeSessionId, surface },
      health,
      hiveIdentity,
      liveFeedState,
      recentEvents,
      runtime,
      sessions,
      workers,
    });
    setPreview(report);
    setCopyState("idle");
    return report;
  }

  async function copyBundle() {
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
    <div className="feedback-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="feedback-heading">
        <header>
          <div><p className="eyebrow">Dogfood feedback</p><h2 id="feedback-heading">Capture what felt wrong</h2></div>
          <button className="secondary-button" type="button" onClick={onClose}>Close</button>
        </header>
        <p>Describe the outcome in your words. Swarm adds content-free runtime evidence, then shows the complete bundle before anything is copied.</p>
        <div className="feedback-fields">
          <label htmlFor="feedback-expectation">What did you expect?
            <textarea id="feedback-expectation" value={expectation} onChange={(event) => { setExpectation(event.target.value); setPreview(undefined); }} placeholder="The worker should have stayed visible after reload." />
          </label>
          <label htmlFor="feedback-observation">What happened instead?
            <textarea id="feedback-observation" value={observation} onChange={(event) => { setObservation(event.target.value); setPreview(undefined); }} placeholder="The terminal area became blank until I refreshed again." />
          </label>
        </div>
        <small className="privacy-note">Your note is included exactly as entered. Automatic evidence never includes terminal output, task text, paths, credentials, worker names, or raw errors.</small>
        <div className="diagnostic-actions">
          <button type="button" disabled={!runtime.loaded} onClick={buildPreview}>{runtime.loaded ? "Preview bundle" : "Gathering evidence…"}</button>
          <button type="button" className="primary-action" disabled={!runtime.loaded} onClick={() => void copyBundle()}>{copyState === "copied" ? "Copied" : "Copy bundle"}</button>
        </div>
        {copyState === "unavailable" ? <p role="status">Clipboard access is unavailable. Select the preview and copy it manually.</p> : null}
        {preview ? <pre className="diagnostic-preview feedback-preview" aria-label="Dogfood feedback bundle">{preview}</pre> : null}
      </section>
    </div>
  );
}
