import { useRef, useState, type ChangeEvent, type FormEvent } from "react";

import type { TerminalConnectionState } from "./TerminalConnection";

export const MAX_TERMINAL_DRAFT_LENGTH = 16_384;

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

export function composeTerminalSubmission(draft: string): string {
  const normalized = draft.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  if (normalized.includes("\n")) {
    return `\u001b[200~${normalized}\u001b[201~\r`;
  }
  return `${normalized}\r`;
}

interface MobileTerminalComposerProps {
  connectionState: TerminalConnectionState;
  onInput: (text: string) => void;
  keysExpanded?: boolean;
  onKeysExpandedChange?: (expanded: boolean) => void;
  onImage?: (image: File) => Promise<void>;
  attachmentState?: "idle" | "uploading" | "ready" | "error";
}

export function MobileTerminalComposer({ connectionState, onInput, keysExpanded: controlledKeysExpanded, onKeysExpandedChange, onImage, attachmentState = "idle" }: MobileTerminalComposerProps) {
  const [draft, setDraft] = useState("");
  const [localKeysExpanded, setLocalKeysExpanded] = useState(initialMobileKeysVisibility);
  const keysExpanded = controlledKeysExpanded ?? localKeysExpanded;
  const textarea = useRef<HTMLTextAreaElement>(null);
  const imageInput = useRef<HTMLInputElement>(null);
  const connected = connectionState === "connected";

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!connected || draft.length === 0) return;
    onInput(composeTerminalSubmission(draft));
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

  async function chooseImage(event: ChangeEvent<HTMLInputElement>) {
    const image = event.target.files?.[0];
    event.target.value = "";
    if (!connected || !image || !onImage) return;
    await onImage(image);
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
          <input ref={imageInput} hidden type="file" accept="image/png,image/jpeg,image/webp,image/gif" onChange={(event) => void chooseImage(event)} />
          <button type="button" className="terminal-image-button" disabled={!connected || !onImage || attachmentState === "uploading"} onClick={() => imageInput.current?.click()}>{attachmentState === "uploading" ? "Adding…" : "Add image"}</button>
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
