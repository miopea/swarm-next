import { useEffect, useMemo, useRef, useState, type ClipboardEvent } from "react";

import { MobileTerminalComposer } from "./MobileTerminalComposer";
import { TerminalConnection, type TerminalConnectionState } from "./TerminalConnection";
import type { TerminalController } from "./TerminalController";
import { terminalWorkspace } from "./TerminalWorkspace";
import { XtermSurface } from "./XtermSurface";
import { clipboardImage, terminalAttachmentPaste, terminalTextPaste, uploadTerminalImage } from "./TerminalAttachments";

export interface TerminalViewProps {
  session: { session_id: string; running: boolean };
  operatorToken: string;
  onStop: () => void;
  busy: boolean;
  canStop?: boolean;
  mobileKeysVisible?: boolean;
  onMobileKeysVisibleChange?: (visible: boolean) => void;
}

export default function TerminalView({ session, operatorToken, onStop, busy, canStop = true, mobileKeysVisible, onMobileKeysVisibleChange }: TerminalViewProps) {
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
  const [attachmentState, setAttachmentState] = useState<"idle" | "uploading" | "ready" | "error">("idle");

  useEffect(() => {
    const element = mount.current;
    if (!element) return;
    controller.attach(element);
    const subscription = controller.subscribe((state, nextDetail) => {
      setConnectionState(state);
      setDetail(nextDetail);
    });
    return () => {
      subscription.dispose();
      controller.detach();
    };
  }, [controller]);

  async function handlePaste(event: ClipboardEvent<HTMLDivElement>) {
    const image = clipboardImage(event.clipboardData);
    event.preventDefault();
    event.stopPropagation();
    if (!image) {
      const text = event.clipboardData.getData("text/plain");
      if (text) controller.sendInput(terminalTextPaste(text));
      return;
    }
    await addImage(image);
  }

  async function addImage(image: File) {
    setAttachmentState("uploading");
    try {
      const path = await uploadTerminalImage(operatorToken, session.session_id, image);
      controller.sendInput(terminalAttachmentPaste(path));
      setAttachmentState("ready");
    } catch {
      setAttachmentState("error");
    }
  }

  return (
    <div
      className="terminal-panel"
      onKeyDownCapture={(event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "v") event.stopPropagation();
      }}
      onPasteCapture={(event) => void handlePaste(event)}
    >
      <div className="terminal-toolbar">
        <div>
          <strong>{session.session_id}</strong>
          <span className={`connection-state connection-${connectionState}`}>{connectionState.replace("_", " ")}</span>
          {detail && <small>{detail}</small>}
          {attachmentState !== "idle" && (
            <small className={`attachment-state attachment-${attachmentState}`} role="status">
              {attachmentState === "uploading" ? "Adding image…" : attachmentState === "ready" ? "Image added · press Enter when ready" : "Image could not be added"}
            </small>
          )}
        </div>
        {canStop ? (
          <button className="danger-button" onClick={onStop} disabled={busy}>Stop worker</button>
        ) : <span className="protected-worker">Always active</span>}
      </div>
      <div className="terminal-mount" ref={mount} />
      <MobileTerminalComposer connectionState={connectionState} onInput={(text) => controller.sendInput(text)} keysExpanded={mobileKeysVisible} onKeysExpandedChange={onMobileKeysVisibleChange} onImage={addImage} attachmentState={attachmentState} />
    </div>
  );
}
