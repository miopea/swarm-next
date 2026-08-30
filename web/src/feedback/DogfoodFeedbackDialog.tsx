import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent } from "react";
import GithubConnectPanel from "./GithubConnectPanel";

import {
  fetchGithubConnection,
  fetchGithubFeedbackReadiness,
  fetchHistoryDiagnostics,
  fileDogfoodReportOnGithub,
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
import { useModalFocus } from "../shared/useModalFocus";
import UnsavedChangesPrompt from "../shared/UnsavedChangesPrompt";
import { serializeDiagnosticReport, type RuntimeDiagnostics } from "../settings/diagnosticReport";

const MAX_FEEDBACK_IMAGE_BYTES = 5 * 1024 * 1024;

type Props = {
  activeSessionId: string | undefined;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  onClose: () => void;
  onSaved?: () => void;
  operatorToken: string;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  surface: string;
  workers: Worker[];
};

export default function DogfoodFeedbackDialog({ activeSessionId, health, hiveIdentity, liveFeedState, onClose, onSaved, operatorToken, recentEvents, sessions, surface, workers }: Props) {
  const [expectation, setExpectation] = useState("");
  const [observation, setObservation] = useState("");
  const [runtime, setRuntime] = useState<RuntimeDiagnostics>({ loaded: false });
  const [preview, setPreview] = useState<string>();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "unavailable">("idle");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  // Whether this Hive can file anywhere. Undefined until asked, so the dialog
  // never flashes a promise it may not be able to keep.
  const [github, setGithub] = useState<{ configured: boolean; repository: string | null }>();
  // Which account a submission will be filed as. Undefined until asked, so the
  // dialog never claims anonymity or attribution before it knows.
  const [connection, setConnection] = useState<{ connected: boolean; login: string | null }>();
  const [issueUrl, setIssueUrl] = useState<string>();
  const [filingError, setFilingError] = useState<string>();
  const [screenshot, setScreenshot] = useState<File>();
  const [screenshotUrl, setScreenshotUrl] = useState<string>();
  const [imageError, setImageError] = useState<string>();
  const [closeConfirm, setCloseConfirm] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const expectationInput = useRef<HTMLTextAreaElement>(null);
  const dirty = saveState !== "saved" && Boolean(expectation.trim() || observation.trim() || screenshot);
  function requestClose() {
    if (closeConfirm) return setCloseConfirm(false);
    if (dirty) return setCloseConfirm(true);
    onClose();
  }
  const dialog = useModalFocus<HTMLElement>(requestClose, true, expectationInput);

  function markChanged() {
    setPreview(undefined);
    setCopyState("idle");
    setSaveState("idle");
    setIssueUrl(undefined);
    setFilingError(undefined);
    void fetchGithubConnection(operatorToken)
      .then(setConnection)
      .catch(() => setConnection({ connected: false, login: null }));
    void fetchGithubFeedbackReadiness(operatorToken)
      .then(setGithub)
      // A Hive that cannot answer is treated as one that cannot file. Better a
      // dialog that says reports stay here than one that offers a button which
      // fails when pressed.
      .catch(() => setGithub({ configured: false, repository: null }));
  }

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
    markChanged();
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
    const attachmentNote = screenshot ? `\n\nUser screenshot\n- file: ${screenshot.name}\n- media type: ${screenshot.type}\n- bytes: ${screenshot.size}\n- image content is never inspected by Swarm and stays on this device unless the operator explicitly saves this report to the Hive` : "";
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
      const saved = await saveDogfoodReport(operatorToken, {
        expectation,
        observation,
        diagnostic_bundle: report,
        attachment_name: attachmentName,
      });
      setSaveState("saved");
      onSaved?.();
      // SAVED FIRST, ALWAYS. The report is on this Hive before GitHub is
      // attempted, so an outage cannot lose somebody's words — which is the
      // failure this whole feature exists to end.
      if (github?.configured) {
        try {
          const filed = await fileDogfoodReportOnGithub(operatorToken, saved.id);
          setIssueUrl(filed.issue_url);
        } catch (error) {
          setFilingError(
            error instanceof Error ? error.message : "GitHub could not be reached.",
          );
        }
      }
    } catch {
      setSaveState("error");
    }
  }

  return (
    <div className="feedback-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) requestClose(); }}>
      <section ref={dialog} tabIndex={-1} className="feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="feedback-heading" onPaste={pastedImage} onDragOver={(event) => event.preventDefault()} onDrop={droppedImage}>
        <header>
          <div><p className="eyebrow">Dogfood feedback</p><h2 id="feedback-heading">Capture what felt wrong</h2></div>
          <button className="secondary-button" type="button" onClick={requestClose}>Close</button>
        </header>
        <p>Describe the outcome in your words. Swarm adds content-free runtime evidence, then shows the complete bundle before anything is copied.</p>
        <div className="feedback-fields">
          <label htmlFor="feedback-expectation">What did you expect?
            <textarea ref={expectationInput} id="feedback-expectation" value={expectation} onChange={(event) => { setExpectation(event.target.value); markChanged(); }} placeholder="The worker should have stayed visible after reload." />
          </label>
          <label htmlFor="feedback-observation">What happened instead?
            <textarea id="feedback-observation" value={observation} onChange={(event) => { setObservation(event.target.value); markChanged(); }} placeholder="The terminal area became blank until I refreshed again." />
          </label>
        </div>
        <div className="feedback-attachment">
          <div><strong>Screenshot</strong><small>Paste, drop, or choose one image up to 5 MiB. Swarm keeps it local and never captures the terminal automatically.</small></div>
          <input ref={fileInput} type="file" accept="image/*" hidden onChange={(event) => attachScreenshot(event.target.files?.[0])} />
          <button type="button" className="secondary-button" onClick={() => fileInput.current?.click()}>{screenshot ? "Replace screenshot" : "Choose screenshot"}</button>
          {screenshotUrl && screenshot ? <div className="feedback-screenshot-preview"><img src={screenshotUrl} alt="Attached dogfood screenshot" /><span><strong>{screenshot.name}</strong><small>{formatBytes(screenshot.size)} · kept on this device</small></span><button type="button" onClick={saveScreenshot}>Save image</button><button type="button" className="danger-text" onClick={() => { URL.revokeObjectURL(screenshotUrl); setScreenshot(undefined); setScreenshotUrl(undefined); markChanged(); }}>Remove</button></div> : null}
          {imageError ? <p role="alert">{imageError}</p> : null}
        </div>
        <small className="privacy-note">Your note is included exactly as entered. Automatic evidence never includes terminal output, task text, paths, credentials, worker names, or raw errors.</small>
        {closeConfirm ? <UnsavedChangesPrompt label="Discard this feedback?" description="Your notes and attached screenshot have not been saved to the Hive." discardLabel="Discard feedback" onDiscard={onClose} onKeep={() => setCloseConfirm(false)} /> : <div className="diagnostic-actions">
          <button type="button" disabled={!runtime.loaded} onClick={buildPreview}>{runtime.loaded ? "Preview bundle" : "Gathering evidence…"}</button>
          <button type="button" className="primary-action" disabled={!runtime.loaded} onClick={() => void copyBundle()}>{copyState === "copied" ? "Copied" : "Copy notes & diagnostics"}</button>
          <button type="button" className="primary-action" disabled={!runtime.loaded || (!expectation.trim() && !observation.trim()) || saveState === "saving" || saveState === "saved"} onClick={() => void saveToHive()}>{saveState === "saving" ? (github?.configured ? "Sending…" : "Saving…") : saveState === "saved" ? (github?.configured ? "Sent" : "Saved to Hive") : github?.configured ? "Send to GitHub" : "Save to this Hive"}</button>
        </div>}
        {/* WHICH OF THE TWO PATHS THIS SUBMISSION TAKES, said before it is taken
            rather than discovered afterwards. Only when the Hive can file at
            all: on a Hive with no GitHub the report stays local and offering to
            connect an account would promise something it cannot do.

            Deliberately below the buttons and never in front of them —
            submitting must not wait on connecting. */}
        {github?.configured && saveState !== "saved" ? (
          <GithubConnectPanel
            operatorToken={operatorToken}
            connection={connection}
            onChanged={() => {
              void fetchGithubConnection(operatorToken)
                .then(setConnection)
                .catch(() => setConnection({ connected: false, login: null }));
            }}
          />
        ) : null}
        {copyState === "unavailable" ? <p role="status">Clipboard access is unavailable. Select the preview and copy it manually.</p> : null}
        {saveState === "saved" ? (
          issueUrl ? (
            // WHERE IT WENT, in the one place the person who sent it is looking.
            <p role="status">Filed on GitHub: <a href={issueUrl} target="_blank" rel="noreferrer">{issueUrl}</a>. Your screenshot stayed on this device.</p>
          ) : filingError ? (
            <p role="status">Saved on this Hive, but GitHub could not be reached: {filingError} Nothing is lost — open Settings, then Saved dogfood reports, to send it later.</p>
          ) : (
            // SAYS SO PLAINLY. It always did only this; what was wrong was a
            // screen that let a person believe otherwise. A colleague pressed
            // Save, thought she had raised an issue, and her report sat here.
            <p role="status">Saved on this Hive and sent nowhere else. Open Settings, then Saved dogfood reports, to review the bundle or download its screenshot.</p>
          )
        ) : null}
        {saveState === "error" ? <p role="alert">The report was not saved. Your notes and screenshot remain in this dialog.</p> : null}
        {preview ? <pre className="diagnostic-preview feedback-preview" aria-label="Dogfood feedback bundle">{preview}</pre> : null}
      </section>
    </div>
  );
}

function formatBytes(bytes: number) {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`;
}
