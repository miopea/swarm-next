import { useEffect, useMemo, useRef, useState, type ClipboardEvent, type DragEvent } from "react";

import { MobileTerminalComposer } from "./MobileTerminalComposer";
import { recordOperatorSubmission } from "./OperatorSubmission";
import { TerminalConnection, type TerminalConnectionState, type TerminalControlView } from "./TerminalConnection";
import type { TerminalController } from "./TerminalController";
import { terminalWorkspace } from "./TerminalWorkspace";
import { XtermSurface } from "./XtermSurface";
import { chosenAttachment, dragCarriesFiles, terminalAttachmentPaste, terminalTextPaste, transferredAttachment, uploadTerminalAttachment } from "./TerminalAttachments";
import type { QueenAutonomyLevel, QueenAutomationStatus, SessionSummary } from "../api";
import { queenAutonomyDetail, queenAutonomyLabel } from "../orchestration/queenAutonomyPresentation";
import { queenAutomationCompactLabel, queenAutomationStateDetail, queenAutomationStateTone } from "../orchestration/queenAutomationPresentation";

export interface TerminalViewProps {
  session: SessionSummary;
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
  const [control, setControl] = useState<TerminalControlView>("checking");
  const connectionStateRef = useRef<TerminalConnectionState>("connecting");
  const attachmentGeneration = useRef(0);
  const uploadRequest = useRef<AbortController | undefined>(undefined);
  const uploadDeadline = useRef<number | undefined>(undefined);
  const waitingAttachment = useRef(false);
  const [selectedFile, setSelectedFile] = useState<File>();
  const [detail, setDetail] = useState<string>();
  const [attachmentState, setAttachmentState] = useState<
    "idle" | "uploading" | "waiting" | "ready" | "error"
  >("idle");
  // THE UPLOADED PATH, HELD UNTIL THERE IS A SOCKET TO PASTE IT DOWN.
  // Uploading is plain HTTP and never needed the connection; only this
  // last step does. Without somewhere to keep the path, a file that
  // uploaded perfectly while the socket was down was reported "ready"
  // and pasted nowhere.
  const [pendingPaste, setPendingPaste] = useState<{ path: string; sessionId: string; controller: TerminalController }>();
  // NAMED IN THE CONFIRMATION. "File added" over a terminal the operator just
  // dropped something into is nearly contentless; the name is what tells them
  // the right file landed.
  const [attachmentName, setAttachmentName] = useState<string>();
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
    setPendingPaste(undefined);
    waitingAttachment.current = false;
    uploadRequest.current = undefined;
    setSelectedFile(undefined);
    setAttachmentName(undefined);
    setAttachmentError(undefined);
    setAttachmentState("idle");
    controller.attach(element);
    const subscription = controller.subscribe((state, nextDetail) => {
      connectionStateRef.current = state;
      setConnectionState(state);
      setDetail(nextDetail);
      report.current?.(state);
    });
    const scrollSubscription = controller.subscribeScroll(setAtBottom);
    const controlSubscription = controller.subscribeControl(setControl);
    // Ctrl+F is taken at the terminal surface, because that is where the key
    // arrives — the panel never sees it once xterm has focus.
    const findSubscription = controller.subscribeFind(() => setFinding(true));
    return () => {
      attachmentGeneration.current += 1;
      uploadRequest.current?.abort();
      window.clearTimeout(uploadDeadline.current);
      connectionStateRef.current = "closed";
      subscription.dispose();
      scrollSubscription.dispose();
      controlSubscription.dispose();
      findSubscription.dispose();
      controller.detach();
    };
  }, [controller, session.session_id]);

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
    setAttachmentState((state) => (state === "ready" ? "idle" : state));
  }

  function removeAttachment() {
    attachmentGeneration.current += 1;
    uploadRequest.current?.abort();
    uploadRequest.current = undefined;
    window.clearTimeout(uploadDeadline.current);
    waitingAttachment.current = false;
    setPendingPaste(undefined);
    setSelectedFile(undefined);
    setAttachmentName(undefined);
    setAttachmentError(undefined);
    setAttachmentState("idle");
  }

  async function handlePaste(event: ClipboardEvent<HTMLDivElement>) {
    const transferred = transferredAttachment(event.clipboardData);
    // Native text editing owns composer/search paste. Only terminal-surface
    // paste should bypass the draft and write directly to the PTY.
    if (transferred.kind === "none" && event.target instanceof HTMLElement
      && event.target.closest(".mobile-terminal-composer, .terminal-find")) return;
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
    // One owned selection, including all paste/drop/picker entry points.
    if (uploadRequest.current || waitingAttachment.current) return;
    const generation = ++attachmentGeneration.current;
    const request = new AbortController();
    uploadRequest.current = request;
    setSelectedFile(file);
    const deadline = window.setTimeout(() => request.abort(), 60_000);
    uploadDeadline.current = deadline;
    setAttachmentState("uploading");
    setAttachmentError(undefined);
    setAttachmentName(file.name);
    try {
      const path = await uploadTerminalAttachment(operatorToken, session.session_id, file, request.signal);
      if (generation !== attachmentGeneration.current) return;
      request.signal.throwIfAborted();
      // READ THE CONNECTION AT THE MOMENT OF PASTING, not before uploading. A
      // phone backgrounds this tab to open its file picker and the socket
      // drops, so the state that mattered when the operator tapped is not the
      // state that matters now -- and an upload of any size gives the
      // reconnect time to finish, or not.
      if (connectionStateRef.current === "connected" && controller.sendInput(terminalAttachmentPaste(path))) {
        setAttachmentState("ready");
        setSelectedFile(undefined);
        return;
      }
      // TerminalConnection#send DISCARDS ANYTHING SENT WHILE THE SOCKET IS
      // CLOSED, silently and by design. Calling it here would report success
      // for a paste that never happened, which is worse than the silence this
      // change exists to remove: the operator would be told the file arrived.
      setPendingPaste({ path, sessionId: session.session_id, controller });
      waitingAttachment.current = true;
      setAttachmentState("waiting");
    } catch (error) {
      if (generation !== attachmentGeneration.current) return;
      setAttachmentError(request.signal.aborted ? "Upload timed out; your selected file is available to retry" : error instanceof Error ? error.message : undefined);
      setAttachmentState("error");
    } finally {
      window.clearTimeout(deadline);
      if (uploadRequest.current === request) uploadRequest.current = undefined;
    }
  }

  // Delivered when the connection returns, which is what the operator wanted
  // when they picked the file. Nothing about an uploaded path expires.
  useEffect(() => {
    if (connectionState !== "connected" || pendingPaste === undefined) return;
    if (pendingPaste.sessionId !== session.session_id || pendingPaste.controller !== controller) return;
    if (!controller.sendInput(terminalAttachmentPaste(pendingPaste.path))) return;
    waitingAttachment.current = false;
    setPendingPaste(undefined);
    setSelectedFile(undefined);
    setAttachmentState("ready");
  }, [connectionState, control, pendingPaste, controller, session.session_id]);

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
  const quiet = connectionState === "connected" && control === "owned" && !detail && attachmentState === "idle";
  const attachmentWaitReason = connectionState === "connected" && control !== "owned"
    ? "Resume Here to add it to this terminal"
    : "waiting for the connection to add it";

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
          <details className={`terminal-session-details${session.recovery_outcome || session.recovery_attempt ? " has-recovery" : ""}`}>
            <summary>Session details</summary>
            <div className="terminal-session-popover">
              <strong>Swarm terminal session</strong>
              <code>{session.session_id}</code>
              <small>This identifies the durable terminal for diagnostics. Your Claude or Codex conversation is separate.</small>
              {session.confirmed_selection && <>
                <strong>Current conversation confirmed</strong>
                <small>Your later conversation selection was confirmed and saved for the next start.</small>
                <code>{session.confirmed_selection.conversation}</code>
              </>}
              {session.recovery_outcome && <>
                <strong>{session.confirmed_selection ? "Earlier startup recovery" : "Conversation recovery result"}</strong>
                <small>{session.confirmed_selection
                  ? `Startup result: ${session.recovery_outcome.state}. The later confirmed selection above supersedes it.`
                  : session.recovery_outcome.state === "restored"
                  ? "Provider context was restored at startup. That conversation was saved as the resumption default."
                  : session.recovery_outcome.state === "fresh"
                    ? "A fresh conversation started after recovery attempts. Previous context was not restored. Use the provider's resume command to choose another conversation."
                    : "Swarm could not confirm the intended conversation. The saved default was not changed. Check this terminal and use the provider's resume command if needed."}</small>
                {session.recovery_outcome.state !== "manual" && <code>{session.recovery_outcome.conversation}</code>}
              </>}
              {!session.recovery_outcome && !session.confirmed_selection && session.recovery_attempt && <>
                <strong>Conversation recovery startup</strong>
                <small>{session.recovery_attempt.step.kind === "continue"
                  ? "Started with provider-native continuation after the saved conversation was unavailable. Swarm has not verified which conversation was restored."
                  : session.recovery_attempt.step.kind === "fresh"
                    ? "Started with a fresh-context attempt. Prior conversation recovery is not confirmed; use the provider's resume command if needed."
                    : "Started with the selected conversation. Process startup alone does not confirm restored context."}</small>
                <small>Recovery attempt {session.recovery_attempt.number} · {session.recovery_attempt.recovery_id}</small>
              </>}
              <button type="button" className="secondary-button" onClick={() => void copySessionId()}>{sessionCopyState === "copied" ? "Copied" : "Copy session ID"}</button>
              {sessionCopyState === "error" ? <small role="alert">Copy was blocked. Select the ID above to copy it manually.</small> : null}
            </div>
          </details>
          {detail && <small>{detail}</small>}
          {!session.confirmed_selection && !session.recovery_outcome && session.recovery_attempt?.step.kind === "continue" && <small>Continuation fallback · see Session details</small>}
          {!session.confirmed_selection && session.recovery_outcome?.state === "manual" && <small role="status">Check conversation · see Session details</small>}
          {!session.confirmed_selection && session.recovery_outcome?.state === "fresh" && <small role="status">Fresh conversation · previous context not restored</small>}
          {control !== "owned" && <small role="status">{control === "unsupported" ? "Viewing only · a safe worker-engine update is needed for terminal control." : control === "checking" ? "Checking terminal control…" : control === "elsewhere" ? "Viewing only · another view controls this terminal." : "Viewing only · ready to resume here."}</small>}
          {(control === "elsewhere" || control === "available") && <button type="button" className="secondary-button" onClick={() => controller.resumeHere()}>Resume Here</button>}
          {attachmentState !== "idle" && (
            <small className={`attachment-state attachment-${attachmentState}`} role="status">
              {attachmentState === "uploading" ? `Adding ${attachmentName ?? "file"}…` : attachmentState === "waiting" ? `${attachmentName ?? "File"} uploaded · ${attachmentWaitReason}` : attachmentState === "ready" ? `Added ${attachmentName ?? "file"} · press Enter to send` : attachmentError ? `Could not add ${attachmentName ?? "file"} — ${attachmentError}. Try again.` : `Could not add ${attachmentName ?? "file"}. Try again.`}
            </small>
          )}
          {selectedFile && <small className="attachment-selection">Selected file · {Math.max(1, Math.ceil(selectedFile.size / 1024))} KB</small>}
          {attachmentState === "error" && selectedFile && <button type="button" className="secondary-button" onClick={() => void addAttachment(selectedFile)}>Retry attachment</button>}
          {(attachmentState === "uploading" || attachmentState === "waiting" || attachmentState === "error") && <button type="button" className="secondary-button" onClick={removeAttachment}>{attachmentState === "uploading" ? "Cancel attachment" : "Remove attachment"}</button>}
          {attachmentState === "ready" && <button type="button" className="secondary-button" onClick={dismissAttachmentNotice}>Dismiss attachment notice</button>}
        </div>
        {/* Sleep lives in the worker-list menu only. A destructive control on
            the terminal bar is prime space spent on something rarely used, and
            the cost of reaching for it by mistake is a stopped worker. */}
        <div className="terminal-worker-controls">
          {connectionState === "recovery_required" && onRefresh && <button type="button" disabled={busy} onClick={onRefresh}>Reload terminal view</button>}
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
      <MobileTerminalComposer key={session.session_id} sessionId={session.session_id} connectionState={connectionState} inputAvailable={control === "owned"} onInput={(text) => { const accepted = controller.sendInput(text); if (accepted && text.includes("\r")) dismissAttachmentNotice(); return accepted; }} onRecordSubmission={(text, signal) => recordOperatorSubmission(operatorToken, session.session_id, text, signal)} keysExpanded={mobileKeysVisible} onKeysExpandedChange={onMobileKeysVisibleChange} onAttachment={acceptChosenFile} attachmentState={attachmentState} onRefresh={onRefresh} />
    </div>
  );
}
