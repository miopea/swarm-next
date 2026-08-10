import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";

import type { TerminalSnapshot } from "./TerminalConnection";
import type { Disposable, TerminalSurface } from "./TerminalController";

export class XtermSurface implements TerminalSurface {
  readonly #terminal = new Terminal({
    cursorBlink: true,
    convertEol: false,
    fontFamily: '"Cascadia Code", "SFMono-Regular", Consolas, monospace',
    fontSize: 14,
    scrollback: 1_000,
    theme: {
      background: "#090d14",
      foreground: "#e5e7eb",
      cursor: "#93c5fd",
      selectionBackground: "#1d4ed866",
    },
  });
  readonly #fit = new FitAddon();
  #resizeObserver: ResizeObserver | undefined;

  constructor() {
    this.#terminal.loadAddon(this.#fit);
  }

  open(element: HTMLElement): void {
    this.#terminal.open(element);
    this.#fit.fit();
    this.#resizeObserver = new ResizeObserver(() => this.#fit.fit());
    this.#resizeObserver.observe(element);
  }

  size(): { rows: number; columns: number } {
    return { rows: this.#terminal.rows, columns: this.#terminal.cols };
  }

  write(bytes: Uint8Array): Promise<void> {
    return new Promise((resolve) => this.#terminal.write(bytes, resolve));
  }

  restore(snapshot: TerminalSnapshot): Promise<void> {
    this.#terminal.reset();
    this.#terminal.resize(snapshot.columns, snapshot.rows);
    return this.write(snapshot.bytes);
  }

  onData(listener: (text: string) => void): Disposable {
    return this.#terminal.onData(listener);
  }

  onResize(listener: (size: { rows: number; columns: number }) => void): Disposable {
    return this.#terminal.onResize(({ rows, cols }) => listener({ rows, columns: cols }));
  }

  dispose(): void {
    this.#resizeObserver?.disconnect();
    this.#terminal.dispose();
  }
}
