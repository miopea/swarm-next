import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

import { renewApiaryWatch } from "../api";
import { WatchStream, type WatchStreamState } from "./WatchStream";
import { approximateRemaining } from "./leaseTime";
import { observeMirrorFit } from "./mirrorScale";

/**
 * What a window draws into. An interface so the window can be tested without
 * standing up a real terminal emulator, and so the emulator stays replaceable.
 */
export interface WatchSurface {
  write(bytes: Uint8Array): void;
  resize(rows: number, columns: number): void;
  clear(): void;
  dispose(): void;
}

type Props = {
  watchId: string;
  operatorToken: string;
  hiveName: string;
  onClose: () => void;
  /**
   * Escalate from watching to controlling, when the Keeper is allowed to.
   *
   * ⚠️ OFFERED HERE BECAUSE THIS IS WHERE THE DECISION IS MADE. Watching is how
   * an operator finds out something needs hands on it; making them close the
   * window, find the row again and press a different button is asking them to
   * navigate at the exact moment they have decided to act. Omitted when there
   * is nothing to escalate to.
   */
  onTakeOver?: () => void;
  /** Injected by tests; the default builds an xterm surface. */
  createSurface?: (host: HTMLElement) => WatchSurface;
};

/** A fifth of the lease, so four renewals can fail before the window lapses. */
export const WATCH_RENEWAL_INTERVAL_MS = 60_000;

const label: Record<WatchStreamState, string> = {
  connecting: "Opening the window…",
  live: "Live",
  closed: "Closed",
};

/**
 * A live, read-only window into another Hive.
 *
 * ⚠️ THERE IS NO INPUT PATH HERE, AND THAT IS STRUCTURAL RATHER THAN STYLISTIC.
 * Watching is looking; typing into another operator's machine is TAKEOVER, which
 * ADR 0036 governs separately and which requires a reasoned, audited, exclusive
 * lease. No keyboard handler, no send method, nothing to accidentally wire up.
 *
 * ⚠️ IT ALSO KEEPS NOTHING. The surface holds what a terminal holds and the
 * window discards it on close, because ADR 0107's bargain is that a watch shows
 * what is on screen now rather than accumulating a record of someone's machine.
 */
export default function WatchWindow({ watchId, operatorToken, hiveName, onClose, onTakeOver, createSurface }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<WatchStreamState>("connecting");
  const [detail, setDetail] = useState<string>();
  const [expiresAt, setExpiresAt] = useState<number>();

  // ⚠️ A WATCH LAPSES FIVE MINUTES AFTER IT OPENS UNLESS SOMETHING RENEWS IT,
  // and for its whole life nothing did: every window closed itself mid-look.
  // Renewal is owned by this window and ends with it, which is the property
  // that keeps a forgotten watch from becoming standing surveillance.
  useEffect(() => {
    let cancelled = false;
    const renew = async () => {
      try {
        const watch = await renewApiaryWatch(operatorToken, watchId);
        if (!cancelled) setExpiresAt(watch.expires_at);
      } catch {
        // The relay ends the stream itself if the watch has truly lapsed; a
        // transient failure is retried on the next tick.
      }
    };
    void renew();
    const timer = window.setInterval(() => void renew(), WATCH_RENEWAL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [watchId, operatorToken]);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    let surface: WatchSurface | undefined;
    const build = createSurface ?? defaultSurface;
    try {
      surface = build(element);
    } catch {
      setState("closed");
      setDetail("This browser could not open a terminal view.");
      return;
    }
    const stream = new WatchStream({
      watchId,
      operatorToken,
      handlers: {
        onOutput: (bytes) => surface?.write(bytes),
        onSnapshot: (snapshot) => {
          surface?.resize(snapshot.rows, snapshot.columns);
          surface?.clear();
          surface?.write(snapshot.bytes);
        },
        onState: (next, why) => {
          setState(next);
          setDetail(why);
        },
      },
    });
    void stream.open();
    // ⚠️ THE MIRROR IS SCALED, NOT REFLOWED. Without this the watched Hive's
    // terminal sat at its own pixel size in whatever room this window had and
    // never changed — the operator's "it doesn't do a redraw like it does on
    // mobile". Reflowing it to fit is the one thing that must not happen here:
    // this window does not own that grid.
    const stopFitting = frame.current
      ? observeMirrorFit(frame.current, element)
      : () => {};
    return () => {
      stopFitting();
      stream.close();
      surface?.dispose();
    };
  }, [watchId, operatorToken, createSurface]);

  // ⚠️ IT FILLS THE SCREEN, AND THAT IS THE POINT. This used to render as a
  // panel inside the Hive roster, a few hundred pixels wide, with somebody
  // else's terminal reflowed into it and no controls but "Close window" — the
  // operator's verdict on 2026-09-22 was "the UI for watching is terrible".
  // A terminal you are reading over someone's shoulder needs the room a
  // terminal needs, and the things you might do next need to be in reach of the
  // moment you decide to do them.
  return (
    <div className="watch-overlay" role="dialog" aria-modal="true" aria-label={`Live window into ${hiveName}`}>
      <section className="watch-window">
        <header>
          <div>
            <p className="eyebrow">Watching</p>
            <h4>{hiveName}</h4>
          </div>
          <span className={`watch-window-state ${state}`} role="status">{detail ?? label[state]}</span>
          <span className="watch-window-tools">
            {onTakeOver ? <button type="button" className="hive-takeover-button" onClick={onTakeOver}>Take over</button> : null}
            <button type="button" className="secondary-button" onClick={onClose}>Stop watching</button>
          </span>
        </header>
        {/* Read-only. Nothing typed here goes anywhere, because nothing listens. */}
        <div className="watch-window-frame" ref={frame}>
          <div className="watch-window-surface" ref={host} />
        </div>
        <small>A live view. Nothing here is recorded, and {hiveName} is showing that you are watching. Stopping takes the notice off their screen.</small>
        {expiresAt !== undefined ? (
          <small>Stays open while this window does. If it is left behind, it closes by itself in about {approximateRemaining(expiresAt)}.</small>
        ) : null}
      </section>
    </div>
  );
}

function defaultSurface(host: HTMLElement): WatchSurface {
  // `disableStdin` is belt and braces beside there being no input path: a
  // terminal that quietly accepted keystrokes and dropped them would read as
  // broken rather than as read-only.
  const terminal = new Terminal({ disableStdin: true, cursorBlink: false });
  terminal.open(host);
  const decoder = new TextDecoder();
  return {
    write: (bytes) => terminal.write(decoder.decode(bytes, { stream: true })),
    // xterm takes columns first. Swapped here rather than at every call site,
    // because every frame the relay carries names rows first.
    resize: (rows, columns) => terminal.resize(Math.max(columns, 1), Math.max(rows, 1)),
    clear: () => terminal.reset(),
    dispose: () => terminal.dispose(),
  };
}
