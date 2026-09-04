import { useEffect, useLayoutEffect, useRef, useState, type ChangeEvent, type FormEvent } from "react";

import { TERMINAL_ATTACHMENT_ACCEPT } from "./TerminalAttachments";
import type { TerminalConnectionState } from "./TerminalConnection";

export const MAX_TERMINAL_DRAFT_LENGTH = 16_384;
export const MOBILE_SUBMIT_KEY_DELAY_MS = 75;
/**
 * How long to wait, after the page is visible again, before deciding a pick
 * produced nothing. Long enough that a `change` event still in flight wins.
 */
export const PICKER_RETURN_GRACE_MS = 1_500;
const PICKER_PENDING_KEY = "swarm.mobile-picker.pending.v1";

function pickerWasInterrupted(): boolean {
  try {
    const raw = window.sessionStorage.getItem(PICKER_PENDING_KEY);
    if (raw === null) return false;
    const at = Number(raw);
    return Number.isFinite(at) && at <= Date.now() && Date.now() - at < 5 * 60_000;
  } catch { return false; }
}

function rememberPicker(pending: boolean) {
  try {
    if (pending) window.sessionStorage.setItem(PICKER_PENDING_KEY, String(Date.now()));
    else window.sessionStorage.removeItem(PICKER_PENDING_KEY);
  } catch { /* Picker operation must not depend on storage permission. */ }
}

export const MOBILE_TERMINAL_KEYS = {
  up: "\u001b[A",
  down: "\u001b[B",
  right: "\u001b[C",
  left: "\u001b[D",
  escape: "\u001b",
  enter: "\r",
  tab: "\t",
  interrupt: "\u0003",
  modeCycle: "\u001b[Z",
} as const;

const MOBILE_KEYS_VISIBILITY = "swarm-next-mobile-keys-expanded";

export function initialMobileKeysVisibility(): boolean {
  return localStorage.getItem(MOBILE_KEYS_VISIBILITY) !== "false";
}

export function rememberMobileKeysVisibility(visible: boolean): void {
  localStorage.setItem(MOBILE_KEYS_VISIBILITY, String(visible));
}

export function composeTerminalSubmission(draft: string): readonly [string, string] {
  const normalized = draft.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  return [`\u001b[200~${normalized}\u001b[201~`, MOBILE_TERMINAL_KEYS.enter];
}

interface MobileTerminalComposerProps {
  connectionState: TerminalConnectionState;
  inputAvailable?: boolean;
  onInput: (text: string) => boolean;
  /** Records authored text separately; never waits before terminal input. */
  onRecordSubmission?: (text: string, signal: AbortSignal) => Promise<void>;
  keysExpanded?: boolean;
  onKeysExpandedChange?: (expanded: boolean) => void;
  onAttachment?: (file: File) => Promise<void>;
  attachmentState?: "idle" | "uploading" | "waiting" | "ready" | "error";
  /** Rebuilds this screen's view of the session. Sends the worker nothing. */
  onRefresh?: () => void;
}

export function MobileTerminalComposer({ connectionState, inputAvailable = true, onInput, onRecordSubmission, keysExpanded: controlledKeysExpanded, onKeysExpandedChange, onAttachment, attachmentState = "idle", onRefresh }: MobileTerminalComposerProps) {
  const [draft, setDraft] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [submissionWarning, setSubmissionWarning] = useState<string>();
  const [sourceWarning, setSourceWarning] = useState<string>();
  const sourceRequests = useRef(new Map<AbortController, number>());
  const sourceGeneration = useRef(0);
  const submitTimer = useRef<number | undefined>(undefined);
  const currentInput = useRef(onInput);
  const connectedRef = useRef(connectionState === "connected" && inputAvailable);
  const [localKeysExpanded, setLocalKeysExpanded] = useState(initialMobileKeysVisibility);
  const keysExpanded = controlledKeysExpanded ?? localKeysExpanded;
  const textarea = useRef<HTMLTextAreaElement>(null);
  const attachmentInput = useRef<HTMLInputElement>(null);
  // WHETHER A PICK IS IN FLIGHT, so that a picker which comes back with nothing
  // is distinguishable from one that was never opened.
  //
  // The operator reported attaching working "about half the time" and, after
  // two fixes, still saw NOTHING AT ALL on the failures — no notice, no error.
  // Every path that reaches the handler now reports something, so silence means
  // the handler is not being reached: `change` fired with an empty list, or it
  // never fired because the page did not survive the round trip. Neither is
  // visible from inside without asking the question before leaving.
  const awaitingPick = useRef(false);
  const [pickerReturnedNothing, setPickerReturnedNothing] = useState(pickerWasInterrupted);
  const [pickerUploadFailed, setPickerUploadFailed] = useState(false);
  const connected = connectionState === "connected" && inputAvailable;

  useLayoutEffect(() => {
    currentInput.current = onInput;
    connectedRef.current = connected;
    if (!connected && submitTimer.current !== undefined) {
      window.clearTimeout(submitTimer.current);
      submitTimer.current = undefined;
      setSubmitting(false);
      setSubmissionWarning("Terminal access changed before submission finished. Text may already be in the terminal; inspect it before sending again.");
    }
  }, [connected, onInput]);

  useEffect(() => () => {
    window.clearTimeout(submitTimer.current);
    submitTimer.current = undefined;
    sourceGeneration.current += 1;
    for (const [request, timer] of sourceRequests.current) {
      window.clearTimeout(timer);
      request.abort();
    }
    sourceRequests.current.clear();
  }, []);

  async function recordSource(text: string) {
    if (!onRecordSubmission) return;
    const generation = ++sourceGeneration.current;
    setSourceWarning(undefined);
    const warn = () => {
      if (generation === sourceGeneration.current) setSourceWarning("Your message's operator-source record could not be confirmed. Terminal sending is separate; do not resend just to fix this record.");
    };
    if (sourceRequests.current.size >= 4) { warn(); return; }
    const controller = new AbortController();
    const timer = window.setTimeout(() => { controller.abort(); warn(); }, 8_000);
    sourceRequests.current.set(controller, timer);
    try { await onRecordSubmission(text, controller.signal); }
    catch { if (!controller.signal.aborted) warn(); }
    finally {
      window.clearTimeout(timer);
      sourceRequests.current.delete(controller);
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!connected || submitting || draft.length === 0 || attachmentState === "uploading" || attachmentState === "waiting" || attachmentState === "error") return;
    const [content, submitKey] = composeTerminalSubmission(draft);
    // Provider TUIs distinguish pasted text from an Enter key event. Keep
    // these as separate WebSocket frames with a brief bounded pause so Codex's
    // paste-burst guard sees a human-style submit instead of leaving the text
    // in its prompt. Claude accepts the same terminal semantics, including
    // bracketed multiline paste.
    setSubmissionWarning(undefined);
    if (!onInput(content)) {
      setSubmissionWarning("The connection did not accept your text. Your draft is still here.");
      return;
    }
    void recordSource(draft);
    setSubmitting(true);
    // Existing provider paste pacing is not proof of delivery. Every write
    // still checks the current socket, and disposal cancels this pending key.
    submitTimer.current = window.setTimeout(() => {
      submitTimer.current = undefined;
      setSubmitting(false);
      if (!connectedRef.current || !currentInput.current(submitKey)) {
        setSubmissionWarning("Submission could not be confirmed. Text may already be in the terminal; inspect it before sending again.");
        return;
      }
      setDraft("");
      textarea.current?.focus();
    }, MOBILE_SUBMIT_KEY_DELAY_MS);
  }

  function sendKey(value: string) {
    if (connected) onInput(value);
  }

  function toggleKeys() {
    const next = !keysExpanded;
    rememberMobileKeysVisibility(next);
    setLocalKeysExpanded(next);
    onKeysExpandedChange?.(next);
  }

  async function chooseAttachment(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    // NO `connected` CHECK, and its absence is the fix. Uploading is plain
    // HTTP and never needed the socket; only pasting the path down the
    // terminal does, and that is handled where the paste happens.
    //
    // The old guard refused here, silently, and the operator reported it as
    // "works about half the time" -- because opening a phone's file picker
    // BACKGROUNDS this tab, the socket drops, and whether the reconnect beat
    // their thumb decided whether the file survived. Nothing said so.
    awaitingPick.current = false;
    rememberPicker(false);
    if (!onAttachment) return;
    if (!file) {
      // THE LAST SILENT BRANCH, and it is no longer silent. A change event
      // carrying no file is unusual -- cancelling a picker fires nothing on
      // most platforms -- so saying so is information rather than noise.
      setPickerReturnedNothing(true);
      return;
    }
    setPickerReturnedNothing(false);
    setPickerUploadFailed(false);
    try { await onAttachment(file); }
    catch { setPickerUploadFailed(true); }
  }

  function openPicker() {
    setPickerReturnedNothing(false);
    awaitingPick.current = true;
    rememberPicker(true);
    attachmentInput.current?.click();
  }

  // A PICK THAT NEVER COMES BACK AT ALL. If the page is evicted while the
  // system picker is open -- which a phone does under memory pressure -- the
  // input that held the selection is gone and `change` can never fire. Nothing
  // in this component runs at that point, so the only way to notice is to find
  // the flag still set when the page is next visible.
  useEffect(() => {
    // The content-free marker survives an evicted page; an in-memory ref does not.
    rememberPicker(false);
    let settle: number | undefined;
    const check = () => {
      window.clearTimeout(settle);
      if (document.visibilityState !== "visible" || !awaitingPick.current) return;
      settle = window.setTimeout(() => {
        if (!awaitingPick.current) return;
        awaitingPick.current = false;
        rememberPicker(false);
        setPickerReturnedNothing(true);
      }, PICKER_RETURN_GRACE_MS);
    };
    const cancel = () => {
      awaitingPick.current = false;
      rememberPicker(false);
      window.clearTimeout(settle);
      setPickerReturnedNothing(false);
    };
    const input = attachmentInput.current;
    input?.addEventListener("cancel", cancel);
    document.addEventListener("visibilitychange", check);
    return () => {
      window.clearTimeout(settle);
      input?.removeEventListener("cancel", cancel);
      document.removeEventListener("visibilitychange", check);
    };
  }, []);

  return (
    <section className="mobile-terminal-composer" aria-label="Mobile terminal controls">
      <form onSubmit={submit}>
        <label htmlFor="mobile-terminal-draft">
          Message worker
          <span aria-hidden="true">{draft.length.toLocaleString()} / {MAX_TERMINAL_DRAFT_LENGTH.toLocaleString()}</span>
        </label>
        <textarea
          ref={textarea}
          id="mobile-terminal-draft"
          rows={2}
          maxLength={MAX_TERMINAL_DRAFT_LENGTH}
          value={draft}
          readOnly={submitting}
          onChange={(event) => setDraft(event.target.value.slice(0, MAX_TERMINAL_DRAFT_LENGTH))}
          placeholder="Type or dictate. Slash commands work here."
          autoCapitalize="sentences"
          enterKeyHint="enter"
        />
        <button type="submit" disabled={!connected || submitting || draft.length === 0 || attachmentState === "uploading" || attachmentState === "waiting" || attachmentState === "error"}>{submitting ? "Sending…" : "Send"}</button>
        {!inputAvailable && draft.length > 0 && <p role="status">Your draft stays here while this terminal is viewing only.</p>}
        {submissionWarning ? <p role="status">{submissionWarning}</p> : null}
        {sourceWarning ? <p role="status">{sourceWarning}</p> : null}
      </form>
      <div className="mobile-terminal-key-heading">
        <span>Terminal tools</span>
        <div>
          {/* ACCEPT NAMES WHAT IS IN, because `accept` cannot express what is
              out. A bare file input offers the whole photo library and Take
              Video, and the operator asked what a video was even for. */}
          <input ref={attachmentInput} hidden type="file" accept={TERMINAL_ATTACHMENT_ACCEPT} onChange={(event) => void chooseAttachment(event)} />
          {/* BESIDE Add file, not inside the keys panel it used to live in.
              This is the way out when the VIEW itself has gone wrong — a
              terminal that will not scroll, or is drawn at the wrong size — and
              it was reachable only by first tapping Show keys. The one control
              you need when the screen is broken should not be hidden behind
              another tap on that same broken screen. Operator's request,
              emailed 2026-08-28. It rebuilds this screen's view and changes
              nothing in the worker. */}
          {onRefresh ? (
            <button type="button" className="terminal-refresh-button" onClick={onRefresh}>Refresh</button>
          ) : null}
          <button type="button" className="terminal-image-button" disabled={!onAttachment || attachmentState === "uploading" || attachmentState === "waiting"} onClick={openPicker}>{attachmentState === "uploading" ? "Adding…" : attachmentState === "waiting" ? "Waiting…" : "Add file"}</button>
          <button type="button" className="terminal-keys-toggle" aria-expanded={keysExpanded} onClick={toggleKeys}>{keysExpanded ? "Hide keys" : "Show keys"}</button>
        </div>
        {pickerReturnedNothing && (
          // SAYS THE THING THAT WAS SILENT. It does not diagnose, because from
          // in here the two causes are indistinguishable -- an empty change
          // event and a page that did not survive the picker look identical.
          // It reports what is true and what to do, which is all the operator
          // needs and more than they had.
          <small className="attachment-state attachment-error" role="status">
            No file arrived from the picker. If you chose one, try again — it did not reach this page.
          </small>
        )}
        {pickerUploadFailed ? <small role="status">The selected file could not be handed to the upload. Please try again.</small> : null}
      </div>
      {keysExpanded && <div className="mobile-terminal-keys" aria-label="Terminal keys">
        <div className="terminal-dpad">
          <button type="button" aria-label="Arrow up" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.up)} disabled={!connected}>↑</button>
          <button type="button" aria-label="Arrow left" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.left)} disabled={!connected}>←</button>
          <button type="button" aria-label="Arrow down" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.down)} disabled={!connected}>↓</button>
          <button type="button" aria-label="Arrow right" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.right)} disabled={!connected}>→</button>
        </div>
        <div className="terminal-key-actions">
          <button type="button" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.enter)} disabled={!connected}>Enter</button>
          <button type="button" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.escape)} disabled={!connected}>Esc</button>
          <button type="button" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.tab)} disabled={!connected}>Tab</button>
          <button type="button" className="interrupt-button" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.interrupt)} disabled={!connected}>Ctrl+C</button>
          <button type="button" className="mode-cycle-button" onClick={() => sendKey(MOBILE_TERMINAL_KEYS.modeCycle)} disabled={!connected}>Cycle mode</button>
        </div>
      </div>}
    </section>
  );
}
