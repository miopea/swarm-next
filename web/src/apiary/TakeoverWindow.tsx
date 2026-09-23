import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";
import { observeMirrorFit } from "./mirrorScale";
import { approximateRemaining } from "./leaseTime";

import { requestTakeoverControlGrant } from "../api";

type Props = {
  leaseId: string;
  operatorToken: string;
  hiveName: string;
  onClose: () => void;
  /**
   * When the lease lapses, from the roster's regular refresh.
   *
   * The held Hive already shows this; the Keeper holding it did not, so the
   * one person able to keep it alive could not see how long it had.
   */
  expiresAt?: number;
  /** Injected by tests; the default builds an xterm surface. */
  createSurface?: (host: HTMLElement, onKey: (data: string) => void) => Surface;
};

export interface Surface {
  write(bytes: Uint8Array): void;
  resize(rows: number, columns: number): void;
  clear(): void;
  dispose(): void;
}

const OUTPUT_FRAME_TYPE = 1;
const SNAPSHOT_FRAME_TYPE = 2;
const INPUT_FRAME_TYPE = 9;
/**
 * How long to wait for the target to acknowledge before giving up.
 *
 * The target learns of a takeover on its own federation pass, which is paced at
 * sixty seconds, so anything shorter would routinely declare a healthy takeover
 * dead. The request lease itself lapses at sixty seconds, which is the real
 * ceiling — waiting past it would be waiting for something that cannot arrive.
 */
const ACKNOWLEDGEMENT_WAIT_MS = 60_000;
const ACKNOWLEDGEMENT_POLL_MS = 2_000;

/**
 * Controlling another operator's Hive.
 *
 * ⚠️ THIS ONE TYPES, WHICH IS WHY IT IS GATED AND THE WATCH WINDOW IS NOT. A
 * takeover is a reasoned, acknowledged, exclusive, five-minute lease over that
 * Hive's Queen, and the operator at the other end sees it the whole time and
 * can end it mid-keystroke. If that notice ever stops rendering, this must stop
 * being reachable — they are one release condition, not two features.
 *
 * ⚠️ AND IT STILL KEEPS NOTHING. The screen arrives through the same bounded
 * relay watching uses; nothing is recorded anywhere but the audit, which says
 * who held the Hive and never what they typed.
 */
export default function TakeoverWindow({ leaseId, operatorToken, hiveName, onClose, expiresAt, createSurface }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<"connecting" | "live" | "closed">("connecting");
  const [detail, setDetail] = useState<string>();

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    let socket: WebSocket | undefined;
    let surface: Surface | undefined;
    let closed = false;

    const send = (data: string) => {
      if (socket?.readyState !== WebSocket.OPEN) return;
      const body = new TextEncoder().encode(data);
      const frame = new Uint8Array(1 + body.byteLength);
      frame[0] = INPUT_FRAME_TYPE;
      frame.set(body, 1);
      socket.send(frame);
    };

    void (async () => {
      // ⚠️ A TAKEOVER IS NOT CONTROLLABLE UNTIL THE TARGET ACKNOWLEDGES IT, and
      // the target polls Keeper on its own schedule. Asking once and giving up
      // showed "no longer active" for a takeover that was merely a few seconds
      // old — a working feature reading as a broken one, which is worse than an
      // honest wait.
      let ticket: { grant: string; websocket_path: string } | undefined;
      const deadline = Date.now() + ACKNOWLEDGEMENT_WAIT_MS;
      while (!closed && ticket === undefined) {
        try {
          ticket = await requestTakeoverControlGrant(operatorToken, leaseId);
        } catch {
          if (Date.now() >= deadline) {
            setState("closed");
            setDetail("This Hive did not accept the takeover.");
            return;
          }
          setDetail("Waiting for this Hive to accept…");
          await new Promise((resolve) => { setTimeout(resolve, ACKNOWLEDGEMENT_POLL_MS); });
        }
      }
      if (closed || ticket === undefined) return;
      setDetail(undefined);
      try {
        surface = (createSurface ?? defaultSurface)(element, send);
      } catch {
        setState("closed");
        setDetail("This browser could not open a terminal view.");
        return;
      }
      const url = `${window.location.origin.replace(/^http/, "ws")}${ticket.websocket_path}`;
      socket = new WebSocket(url, [`swarm-takeover.${ticket.grant}`]);
      socket.binaryType = "arraybuffer";
      socket.addEventListener("open", () => setState("live"));
      socket.addEventListener("message", (event) => {
        if (!(event.data instanceof ArrayBuffer)) return;
        const frame = new Uint8Array(event.data);
        if (frame.byteLength < 9) return;
        if (frame[0] === OUTPUT_FRAME_TYPE) {
          surface?.write(frame.slice(9));
        } else if (frame[0] === SNAPSHOT_FRAME_TYPE && frame.byteLength >= 14) {
          const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
          surface?.resize(view.getUint16(9), view.getUint16(11));
          surface?.clear();
          surface?.write(frame.slice(14));
        }
      });
      // ⚠️ "CLOSED", NOT "FAILED". The likeliest reason this ends is the other
      // operator taking their machine back, which they are entitled to do
      // without warning. Calling that a connection error would blame the
      // network for somebody's decision about their own computer.
      const ended = () => { setState("closed"); setDetail("The takeover ended."); };
      socket.addEventListener("close", ended);
      socket.addEventListener("error", ended);
    })();

    // Scaled, never reflowed: the Hive being held owns this grid, exactly as it
    // does while being watched. See `mirrorScale`.
    const stopFitting = frame.current
      ? observeMirrorFit(frame.current, element)
      : () => {};
    return () => {
      stopFitting();
      closed = true;
      socket?.close();
      surface?.dispose();
    };
  }, [leaseId, operatorToken, createSurface]);

  // ⚠️ THE SAME FULL WINDOW WATCHING GETS. Escalating from a watch used to drop
  // the operator from a full-screen window back into a panel a few hundred
  // pixels wide — "when I take over it goes back to a small window" — which is
  // backwards: typing on somebody else's machine needs MORE room to see than
  // reading does, not less.
  return (
    <div className="watch-overlay" role="dialog" aria-modal="true" aria-label={`Controlling ${hiveName}`}>
      <section className="watch-window takeover-window">
        <header>
          <div><p className="eyebrow">Controlling</p><h4>{hiveName}</h4></div>
          <span className={`watch-window-state ${state}`} role="status">{detail ?? (state === "live" ? "Live — you are typing on this Hive" : "Opening…")}</span>
          <span className="watch-window-tools">
            <button type="button" className="secondary-button" onClick={onClose}>Hand back</button>
          </span>
        </header>
        <div className="watch-window-frame" ref={frame}>
          <div className="takeover-window-surface" ref={host} />
        </div>
        <small>
          {expiresAt === undefined ? null : <>Lapses in about {approximateRemaining(expiresAt)} unless you keep typing. </>}
          {hiveName} is showing that you hold it, and can take it back at any moment. Handing back ends the takeover and clears that notice.
        </small>
      </section>
    </div>
  );
}

function defaultSurface(host: HTMLElement, onKey: (data: string) => void): Surface {
  const terminal = new Terminal({ cursorBlink: true });
  terminal.open(host);
  terminal.onData(onKey);
  const decoder = new TextDecoder();
  return {
    write: (bytes) => terminal.write(decoder.decode(bytes, { stream: true })),
    resize: (rows, columns) => terminal.resize(Math.max(columns, 1), Math.max(rows, 1)),
    clear: () => terminal.reset(),
    dispose: () => terminal.dispose(),
  };
}
