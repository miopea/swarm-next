import { useEffect, useMemo, useRef, useState } from "react";

import { TerminalConnection, type TerminalConnectionState } from "./TerminalConnection";
import type { TerminalController } from "./TerminalController";
import { terminalWorkspace } from "./TerminalWorkspace";
import { XtermSurface } from "./XtermSurface";

export interface TerminalViewProps {
  session: { session_id: string; running: boolean };
  operatorToken: string;
  onStop: () => void;
  busy: boolean;
}

export default function TerminalView({ session, operatorToken, onStop, busy }: TerminalViewProps) {
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

  return (
    <div className="terminal-panel">
      <div className="terminal-toolbar">
        <div>
          <strong>{session.session_id}</strong>
          <span className={`connection-state connection-${connectionState}`}>{connectionState.replace("_", " ")}</span>
          {detail && <small>{detail}</small>}
        </div>
        <button className="danger-button" onClick={onStop} disabled={busy}>Stop worker</button>
      </div>
      <div className="terminal-mount" ref={mount} />
    </div>
  );
}
