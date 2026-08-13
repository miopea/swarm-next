import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent } from "react";

import {
  fetchHistoryDiagnostics,
  fetchRuntimeResources,
  fetchTerminalHostStatus,
  saveDogfoodReport,
  uploadDogfoodScreenshot,
  type ControlRoomEvent,
  type Health,
  type HiveIdentity,
  type SessionSummary,
  type Worker,
} from "../api";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import { serializeDiagnosticReport, type RuntimeDiagnostics } from "../settings/diagnosticReport";

const MAX_FEEDBACK_IMAGE_BYTES = 5 * 1024 * 1024;

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
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [screenshot, setScreenshot] = useState<File>();
  const [screenshotUrl, setScreenshotUrl] = useState<string>();
  const [imageError, setImageError] = useState<string>();
  const fileInput = useRef<HTMLInputElement>(null);

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

  useEffect(() => () => { if (screenshotUrl) URL.revokeObjectURL(screenshotUrl); }, [screenshotUrl]);

  function attachScreenshot(file: File | undefined) {
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      setImageError("Choose or paste an image file.");
      return;
    }
    if (file.size > MAX_FEEDBACK_IMAGE_BYTES) {
      setImageError("Keep screenshots under 5 MiB.");
      return;
    }
    if (screenshotUrl) URL.revokeObjectURL(screenshotUrl);
    setScreenshot(file);
    setScreenshotUrl(URL.createObjectURL(file));
    setImageError(undefined);
    setPreview(undefined);
  }

  function pastedImage(event: ClipboardEvent<HTMLElement>) {
    const image = [...event.clipboardData.files].find((file) => file.type.startsWith("image/"));
    if (!image) return;
    event.preventDefault();
    attachScreenshot(image);
  }

  function droppedImage(event: DragEvent<HTMLElement>) {
    event.preventDefault();
    attachScreenshot([...event.dataTransfer.files].find((file) => file.type.startsWith("image/")));
  }

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
    const attachmentNote = screenshot ? `\n\nUser screenshot\n- file: ${screenshot.name}\n- media type: ${screenshot.type}\n- bytes: ${screenshot.size}\n- image content is attached separately and was not inspected or uploaded by Swarm` : "";
    const bundle = `${report}${attachmentNote}`;
    setPreview(bundle);
    setCopyState("idle");
    return bundle;
  }

  function saveScreenshot() {
    if (!screenshot || !screenshotUrl) return;
    const anchor = document.createElement("a");
    anchor.href = screenshotUrl;
    anchor.download = screenshot.name || "swarm-dogfood-screenshot.png";
    anchor.click();
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

  async function saveToHive() {
    if (!expectation.trim() && !observation.trim()) return;
    setSaveState("saving");
    try {
      const report = preview ?? buildPreview();
      const attachmentName = screenshot
        ? await uploadDogfoodScreenshot(operatorToken, screenshot)
        : null;
      await saveDogfoodReport(operatorToken, {
        expectation,
        observation,
        diagnostic_bundle: report,
        attachment_name: attachmentName,
      });
      setSaveState("saved");
    } catch {
      setSaveState("error");
    }
  }

  return (
    <div className="feedback-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="feedback-heading" onPaste={pastedImage} onDragOver={(event) => event.preventDefault()} onDrop={droppedImage}>
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
        <div className="feedback-attachment">
          <div><strong>Screenshot</strong><small>Paste, drop, or choose one image up to 5 MiB. Swarm keeps it local and never captures the terminal automatically.</small></div>
          <input ref={fileInput} type="file" accept="image/*" hidden onChange={(event) => attachScreenshot(event.target.files?.[0])} />
          <button type="button" className="secondary-button" onClick={() => fileInput.current?.click()}>{screenshot ? "Replace screenshot" : "Choose screenshot"}</button>
          {screenshotUrl && screenshot ? <div className="feedback-screenshot-preview"><img src={screenshotUrl} alt="Attached dogfood screenshot" /><span><strong>{screenshot.name}</strong><small>{formatBytes(screenshot.size)} · kept on this device</small></span><button type="button" onClick={saveScreenshot}>Save image</button><button type="button" className="danger-text" onClick={() => { URL.revokeObjectURL(screenshotUrl); setScreenshot(undefined); setScreenshotUrl(undefined); setPreview(undefined); }}>Remove</button></div> : null}
          {imageError ? <p role="alert">{imageError}</p> : null}
        </div>
        <small className="privacy-note">Your note is included exactly as entered. Automatic evidence never includes terminal output, task text, paths, credentials, worker names, or raw errors.</small>
        <div className="diagnostic-actions">
          <button type="button" disabled={!runtime.loaded} onClick={buildPreview}>{runtime.loaded ? "Preview bundle" : "Gathering evidence…"}</button>
          <button type="button" className="primary-action" disabled={!runtime.loaded} onClick={() => void copyBundle()}>{copyState === "copied" ? "Copied" : "Copy notes & diagnostics"}</button>
          <button type="button" className="primary-action" disabled={!runtime.loaded || (!expectation.trim() && !observation.trim()) || saveState === "saving"} onClick={() => void saveToHive()}>{saveState === "saving" ? "Saving…" : saveState === "saved" ? "Saved to Hive" : "Save to this Hive"}</button>
        </div>
        {copyState === "unavailable" ? <p role="status">Clipboard access is unavailable. Select the preview and copy it manually.</p> : null}
        {saveState === "saved" ? <p role="status">Saved privately. Tell your Swarm developer that a dogfood report is ready; they can read it through the authenticated API.</p> : null}
        {saveState === "error" ? <p role="alert">The report was not saved. Your notes and screenshot remain in this dialog.</p> : null}
        {preview ? <pre className="diagnostic-preview feedback-preview" aria-label="Dogfood feedback bundle">{preview}</pre> : null}
      </section>
    </div>
  );
}

function formatBytes(bytes: number) {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`;
}
