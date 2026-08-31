import { useRef, useState, type ChangeEvent, type FormEvent } from "react";

import type { TerminalConnectionState } from "./TerminalConnection";

export const MAX_TERMINAL_DRAFT_LENGTH = 16_384;
export const MOBILE_SUBMIT_KEY_DELAY_MS = 75;

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
  onInput: (text: string) => void;
  keysExpanded?: boolean;
  onKeysExpandedChange?: (expanded: boolean) => void;
  onAttachment?: (file: File) => Promise<void>;
  attachmentState?: "idle" | "uploading" | "waiting" | "ready" | "error";
  /** Rebuilds this screen's view of the session. Sends the worker nothing. */
  onRefresh?: () => void;
}

export function MobileTerminalComposer({ connectionState, onInput, keysExpanded: controlledKeysExpanded, onKeysExpandedChange, onAttachment, attachmentState = "idle", onRefresh }: MobileTerminalComposerProps) {
  const [draft, setDraft] = useState("");
  const [localKeysExpanded, setLocalKeysExpanded] = useState(initialMobileKeysVisibility);
  const keysExpanded = controlledKeysExpanded ?? localKeysExpanded;
  const textarea = useRef<HTMLTextAreaElement>(null);
  const attachmentInput = useRef<HTMLInputElement>(null);
  const connected = connectionState === "connected";

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!connected || draft.length === 0) return;
    const [content, submitKey] = composeTerminalSubmission(draft);
    // Provider TUIs distinguish pasted text from an Enter key event. Keep
    // these as separate WebSocket frames with a brief bounded pause so Codex's
    // paste-burst guard sees a human-style submit instead of leaving the text
    // in its prompt. Claude accepts the same terminal semantics, including
    // bracketed multiline paste.
    onInput(content);
    window.setTimeout(() => onInput(submitKey), MOBILE_SUBMIT_KEY_DELAY_MS);
    setDraft("");
    requestAnimationFrame(() => textarea.current?.focus());
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
    if (!file || !onAttachment) return;
    await onAttachment(file);
  }

  return (
    <section className="mobile-terminal-composer" aria-label="Mobile terminal controls">
      <form onSubmit={submit}>
        <label htmlFor="mobile-terminal-draft">
          Message worker
          <span>{draft.length.toLocaleString()} / {MAX_TERMINAL_DRAFT_LENGTH.toLocaleString()}</span>
        </label>
        <textarea
          ref={textarea}
          id="mobile-terminal-draft"
          rows={2}
          maxLength={MAX_TERMINAL_DRAFT_LENGTH}
          value={draft}
          onChange={(event) => setDraft(event.target.value.slice(0, MAX_TERMINAL_DRAFT_LENGTH))}
          placeholder="Type or dictate. Slash commands work here."
          autoCapitalize="sentences"
          enterKeyHint="enter"
        />
        <button type="submit" disabled={!connected || draft.length === 0}>Send</button>
      </form>
      <div className="mobile-terminal-key-heading">
        <span>Terminal tools</span>
        <div>
          <input ref={attachmentInput} hidden type="file" onChange={(event) => void chooseAttachment(event)} />
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
          <button type="button" className="terminal-image-button" disabled={!onAttachment || attachmentState === "uploading"} onClick={() => attachmentInput.current?.click()}>{attachmentState === "uploading" ? "Adding…" : attachmentState === "waiting" ? "Waiting…" : "Add file"}</button>
          <button type="button" className="terminal-keys-toggle" aria-expanded={keysExpanded} onClick={toggleKeys}>{keysExpanded ? "Hide keys" : "Show keys"}</button>
        </div>
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
