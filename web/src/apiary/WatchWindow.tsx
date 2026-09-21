import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

import { WatchStream, type WatchStreamState } from "./WatchStream";

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
  /** Injected by tests; the default builds an xterm surface. */
  createSurface?: (host: HTMLElement) => WatchSurface;
};

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
export default function WatchWindow({ watchId, operatorToken, hiveName, onClose, createSurface }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<WatchStreamState>("connecting");
  const [detail, setDetail] = useState<string>();

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
    return () => {
      stream.close();
      surface?.dispose();
    };
  }, [watchId, operatorToken, createSurface]);

  return (
    <section className="watch-window" aria-label={`Live window into ${hiveName}`}>
      <header>
        <div>
          <p className="eyebrow">Watching</p>
          <h4>{hiveName}</h4>
        </div>
        <span className={`watch-window-state ${state}`} role="status">{detail ?? label[state]}</span>
        <button type="button" onClick={onClose}>Close window</button>
      </header>
      {/* Read-only. Nothing typed here goes anywhere, because nothing listens. */}
      <div className="watch-window-surface" ref={host} />
      <small>A live view. Nothing here is recorded, and {hiveName} is showing that you are watching.</small>
    </section>
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
