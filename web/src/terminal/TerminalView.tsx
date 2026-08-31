import { useEffect, useMemo, useRef, useState, type ClipboardEvent, type DragEvent } from "react";

import { MobileTerminalComposer } from "./MobileTerminalComposer";
import { TerminalConnection, type TerminalConnectionState } from "./TerminalConnection";
import type { TerminalController } from "./TerminalController";
import { terminalWorkspace } from "./TerminalWorkspace";
import { XtermSurface } from "./XtermSurface";
import { chosenAttachment, dragCarriesFiles, terminalAttachmentPaste, terminalTextPaste, transferredAttachment, uploadTerminalAttachment } from "./TerminalAttachments";
import type { QueenAutonomyLevel, QueenAutomationStatus } from "../api";
import { queenAutonomyDetail, queenAutonomyLabel } from "../orchestration/queenAutonomyPresentation";
import { queenAutomationCompactLabel, queenAutomationStateDetail, queenAutomationStateTone } from "../orchestration/queenAutomationPresentation";

export interface TerminalViewProps {
  session: { session_id: string; running: boolean };
  operatorToken: string;
  busy: boolean;
  canStop?: boolean;
  mobileKeysVisible?: boolean;
  /** Rebuilds this screen's view of the session, for when it has gone wrong. */
  onRefresh?: () => void;
  onMobileKeysVisibleChange?: (visible: boolean) => void;
  queenAutomation?: QueenAutomationStatus;
  queenAutonomy?: QueenAutonomyLevel;
  onOpenQueenSettings?: () => void;
  onConnectionStateChange?: (state: TerminalConnectionState) => void;
}

export default function TerminalView({ session, operatorToken, busy, canStop = true, mobileKeysVisible, onMobileKeysVisibleChange, onRefresh, queenAutomation, queenAutonomy, onOpenQueenSettings, onConnectionStateChange }: TerminalViewProps) {
  const mount = useRef<HTMLDivElement>(null);
  const controller = useMemo<TerminalController>(() => {
    terminalWorkspace.authenticate(operatorToken);
    return terminalWorkspace.controllerFor(
      session.session_id,
      () => new XtermSurface(),
      () => new TerminalConnection({ sessionId: session.session_id, operatorToken }),
    );
  }, [operatorToken, session.session_id]);
  const [connectionState, setConnectionState] = useState<TerminalConnectionState>("connecting");
  const [detail, setDetail] = useState<string>();
  const [attachmentState, setAttachmentState] = useState<
    "idle" | "uploading" | "waiting" | "ready" | "error"
  >("idle");
  // THE UPLOADED PATH, HELD UNTIL THERE IS A SOCKET TO PASTE IT DOWN.
  // Uploading is plain HTTP and never needed the connection; only this
  // last step does. Without somewhere to keep the path, a file that
  // uploaded perfectly while the socket was down was reported "ready"
  // and pasted nowhere.
  const [pendingPaste, setPendingPaste] = useState<string>();
  // Why it failed, not just that it did. "Image could not be added" on its own
  // left an operator with nothing to act on and nothing to report.
  const [attachmentError, setAttachmentError] = useState<string>();
  const [dropActive, setDropActive] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  // Held in a ref so a new callback identity cannot detach and reattach the
  // terminal: connection lifetime must not follow a React prop.
  const report = useRef(onConnectionStateChange);
  useEffect(() => { report.current = onConnectionStateChange; }, [onConnectionStateChange]);
  const [sessionCopyState, setSessionCopyState] = useState<"idle" | "copied" | "error">("idle");
  const [finding, setFinding] = useState(false);
  const [query, setQuery] = useState("");
  const [noMatch, setNoMatch] = useState(false);
  const findInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const element = mount.current;
    if (!element) return;
    controller.attach(element);
    const subscription = controller.subscribe((state, nextDetail) => {
      setConnectionState(state);
      setDetail(nextDetail);
      report.current?.(state);
    });
    const scrollSubscription = controller.subscribeScroll(setAtBottom);
    // Ctrl+F is taken at the terminal surface, because that is where the key
    // arrives — the panel never sees it once xterm has focus.
    const findSubscription = controller.subscribeFind(() => setFinding(true));
    return () => {
      subscription.dispose();
      scrollSubscription.dispose();
      findSubscription.dispose();
      controller.detach();
    };
  }, [controller]);

  useEffect(() => {
    if (finding) findInput.current?.focus();
  }, [finding]);

  function search(direction: "next" | "previous") {
    if (!query) return;
    setNoMatch(!controller.find(query, direction));
  }

  function closeFind() {
    setFinding(false);
    setNoMatch(false);
    controller.requestFocus(true);
  }

  function dismissAttachmentNotice() {
    setAttachmentState((state) => (state === "uploading" ? state : "idle"));
  }

  async function handlePaste(event: ClipboardEvent<HTMLDivElement>) {
    const transferred = transferredAttachment(event.clipboardData);
    event.preventDefault();
    event.stopPropagation();
    if (transferred.kind === "file") return addAttachment(transferred.file);
    if (transferred.kind === "too-large") return refuseSize(transferred.description);
    const text = event.clipboardData.getData("text/plain");
    if (text) controller.sendInput(terminalTextPaste(text));
  }

  /**
   * Dropping a file onto the terminal.
   *
   * This did not exist. Drag-and-drop was never wired to the terminal for any
   * format, so "it won't work" was true of PNGs too — the operator had only
   * ever pasted those, so the gap read as a GIF problem.
   */
  async function handleDrop(event: DragEvent<HTMLDivElement>) {
    if (!dragCarriesFiles(event.dataTransfer)) return;
    event.preventDefault();
    event.stopPropagation();
    setDropActive(false);
    const transferred = transferredAttachment(event.dataTransfer);
    if (transferred.kind === "file") return addAttachment(transferred.file);
    if (transferred.kind === "too-large") refuseSize(transferred.description);
  }

  /**
   * Says what was too big and what the limit is.
   *
   * The server enforces this with a transport body limit, which rejects the
   * upload before any code that could explain it — so an oversized attachment
   * produced a bare transport failure, or nothing legible at all.
   */
  function refuseSize(description: string) {
    setAttachmentError(`${description}. Shrink it, or send a still instead.`);
    setAttachmentState("error");
  }

  /** Says why, rather than doing nothing and looking broken. */

  /**
   * A file the operator picked, judged by the rules a dropped one is judged by.
   *
   * The picker used to hand its file straight to the upload, so an oversized
   * one ran until the server's transport limit killed it — the opaque failure
   * `refuseSize` exists to prevent, reached by the one path that skipped it.
   * A phone video is the ordinary way to meet that, at 150-350 MB a minute.
   */
  async function acceptChosenFile(file: File) {
    const judged = chosenAttachment(file);
    if (judged.kind === "too-large") {
      refuseSize(judged.description);
      return;
    }
    if (judged.kind === "file") await addAttachment(judged.file);
  }

  async function addAttachment(file: File) {
    setAttachmentState("uploading");
    setAttachmentError(undefined);
    try {
      const path = await uploadTerminalAttachment(operatorToken, session.session_id, file);
      // READ THE CONNECTION AT THE MOMENT OF PASTING, not before uploading. A
      // phone backgrounds this tab to open its file picker and the socket
      // drops, so the state that mattered when the operator tapped is not the
      // state that matters now -- and an upload of any size gives the
      // reconnect time to finish, or not.
      if (connectionState === "connected") {
        controller.sendInput(terminalAttachmentPaste(path));
        setAttachmentState("ready");
        return;
      }
      // TerminalConnection#send DISCARDS ANYTHING SENT WHILE THE SOCKET IS
      // CLOSED, silently and by design. Calling it here would report success
      // for a paste that never happened, which is worse than the silence this
      // change exists to remove: the operator would be told the file arrived.
      setPendingPaste(path);
      setAttachmentState("waiting");
    } catch (error) {
      setAttachmentError(error instanceof Error ? error.message : undefined);
      setAttachmentState("error");
    }
  }

  // Delivered when the connection returns, which is what the operator wanted
  // when they picked the file. Nothing about an uploaded path expires.
  useEffect(() => {
    if (connectionState !== "connected" || pendingPaste === undefined) return;
    controller.sendInput(terminalAttachmentPaste(pendingPaste));
    setPendingPaste(undefined);
    setAttachmentState("ready");
  }, [connectionState, pendingPaste, controller]);

  async function copySessionId() {
    try {
      await navigator.clipboard.writeText(session.session_id);
      setSessionCopyState("copied");
    } catch {
      setSessionCopyState("error");
    }
  }

  // On a phone the connection chip and the sleep action live in the workspace
  // header instead. The toolbar then has nothing left to say unless something
  // transient is happening, so it reports that and the layout reclaims the row.
  const quiet = connectionState === "connected" && !detail && attachmentState === "idle";

  return (
    <div
      className="terminal-panel"
      onKeyDownCapture={(event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "v") event.stopPropagation();
        if (event.key === "Enter") dismissAttachmentNotice();
        // Ctrl+F reaches here only when the terminal does not have focus; with
        // focus it is taken at the surface. Both doors open the same bar.
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
          event.preventDefault();
          event.stopPropagation();
          setFinding(true);
        }
      }}
      onPasteCapture={(event) => void handlePaste(event)}
      onDragOver={(event) => {
        if (!dragCarriesFiles(event.dataTransfer)) return;
        // Preventing the default is what makes this a drop target at all.
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
        setDropActive(true);
      }}
      onDragLeave={(event) => {
        if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
        setDropActive(false);
      }}
      onDrop={(event) => void handleDrop(event)}
      data-drop-active={dropActive || undefined}
    >
      <div className={`terminal-toolbar${quiet ? " terminal-toolbar-quiet" : ""}`}>
        <div className="terminal-connection-summary">
          <span className={`connection-state connection-${connectionState}`}>{connectionState.replace("_", " ")}</span>
          <details className="terminal-session-details">
            <summary>Session details</summary>
            <div className="terminal-session-popover">
              <strong>Swarm terminal session</strong>
              <code>{session.session_id}</code>
              <small>This identifies the durable terminal for diagnostics. Your Claude or Codex conversation is separate.</small>
              <button type="button" className="secondary-button" onClick={() => void copySessionId()}>{sessionCopyState === "copied" ? "Copied" : "Copy session ID"}</button>
              {sessionCopyState === "error" ? <small role="alert">Copy was blocked. Select the ID above to copy it manually.</small> : null}
            </div>
          </details>
          {detail && <small>{detail}</small>}
          {attachmentState !== "idle" && (
            <small className={`attachment-state attachment-${attachmentState}`} role="status">
              {attachmentState === "uploading" ? "Adding file…" : attachmentState === "waiting" ? "File uploaded · waiting for the connection to add it" : attachmentState === "ready" ? "File added · press Enter when ready" : attachmentError ? `File could not be added — ${attachmentError}. Try again.` : "File could not be added. Try again."}
            </small>
          )}
        </div>
        {/* Sleep lives in the worker-list menu only. A destructive control on
            the terminal bar is prime space spent on something rarely used, and
            the cost of reaching for it by mistake is a stopped worker. */}
        <div className="terminal-worker-controls">
          {queenAutonomy && onOpenQueenSettings ? (
            <button type="button" className="queen-autonomy-chip" title={queenAutonomyDetail(queenAutonomy)} onClick={onOpenQueenSettings}>
              {queenAutonomyLabel(queenAutonomy)}
            </button>
          ) : null}
          {queenAutomation && onOpenQueenSettings ? (
            <button
              type="button"
              className={`queen-automation-chip ${queenAutomationStateTone(queenAutomation)}`}
              title={queenAutomationStateDetail(queenAutomation, "terminal")}
              onClick={onOpenQueenSettings}
            >
              <span className={`presence ${queenAutomationStateTone(queenAutomation)}`} />
              {queenAutomationCompactLabel(queenAutomation)}
            </button>
          ) : null}
          {!canStop ? <span className="protected-worker">Always active</span> : null}
        </div>
      </div>
      <div className="terminal-stage">
        {finding && (
          <div className="terminal-find" role="search" aria-label="Search this terminal">
            <input
              ref={findInput}
              type="text"
              value={query}
              placeholder="Find in terminal"
              aria-label="Find in terminal"
              onChange={(event) => { setQuery(event.target.value); setNoMatch(false); }}
              onKeyDown={(event) => {
                if (event.key === "Escape") { event.preventDefault(); closeFind(); }
                if (event.key === "Enter") { event.preventDefault(); search(event.shiftKey ? "previous" : "next"); }
              }}
            />
            <button type="button" className="text-button" onClick={() => search("previous")} aria-label="Previous match">↑</button>
            <button type="button" className="text-button" onClick={() => search("next")} aria-label="Next match">↓</button>
            {noMatch ? <small role="status">No match</small> : null}
            <button type="button" className="text-button" onClick={closeFind} aria-label="Close search">Close</button>
          </div>
        )}
        <div className="terminal-mount" ref={mount} />
        {!atBottom ? <button type="button" className="terminal-jump-latest" onClick={() => controller.scrollToBottom()}>Jump to latest ↓</button> : null}
      </div>
      <MobileTerminalComposer connectionState={connectionState} onInput={(text) => { controller.sendInput(text); if (text.includes("\r")) dismissAttachmentNotice(); }} keysExpanded={mobileKeysVisible} onKeysExpandedChange={onMobileKeysVisibleChange} onAttachment={acceptChosenFile} attachmentState={attachmentState} onRefresh={onRefresh} />
    </div>
  );
}
