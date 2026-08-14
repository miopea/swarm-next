import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";

import { documentColorTheme, terminalTheme } from "../brand/terminalTheme";
import type { TerminalSnapshot } from "./TerminalConnection";
import type { Disposable, TerminalSurface } from "./TerminalController";

const MAX_FIT_FRAMES = 60;
const RESIZE_SETTLE_MS = 120;
export const MIN_TERMINAL_ROWS = 4;
export const MIN_TERMINAL_COLUMNS = 20;

export class XtermSurface implements TerminalSurface {
  readonly #terminal: Terminal;
  readonly #fit = new FitAddon();
  readonly #themeObserver: MutationObserver | undefined;
  #resizeObserver: ResizeObserver | undefined;
  #resizeTimer: ReturnType<typeof setTimeout> | undefined;
  #element: HTMLElement | undefined;
  #restorePending = false;
  #disposed = false;

  constructor() {
    this.#terminal = new Terminal({
      cursorBlink: true,
      convertEol: false,
      fontFamily: '"Atkinson Hyperlegible Mono Variable", "Cascadia Code", "SFMono-Regular", Consolas, monospace',
      fontSize: 14,
      minimumContrastRatio: 4.5,
      scrollback: 1_000,
      theme: terminalTheme(documentColorTheme()),
    });
    this.#terminal.loadAddon(this.#fit);
    this.#themeObserver = typeof MutationObserver === "undefined" ? undefined : new MutationObserver(() => {
      this.#terminal.options.theme = terminalTheme(documentColorTheme());
    });
    this.#themeObserver?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  }

  open(element: HTMLElement): void {
    this.#element = element;
    this.#terminal.open(element);
    this.#resizeObserver = new ResizeObserver(() => this.#scheduleFit());
    this.#resizeObserver.observe(element);
  }

  focus(): void {
    if (!this.#disposed) this.#terminal.focus();
  }

  async fit(): Promise<{ rows: number; columns: number }> {
    try {
      if (this.#disposed) throw new Error("Cannot fit a disposed terminal renderer");
      this.#cancelScheduledFit();
      await document.fonts?.ready;
      for (let frame = 0; frame < MAX_FIT_FRAMES; frame += 1) {
        await nextAnimationFrame();
        if (this.#disposed) throw new Error("Cannot fit a disposed terminal renderer");
        const dimensions = this.#fit.proposeDimensions();
        const usable = usableDimensions(dimensions);
        if (!usable) continue;
        this.#terminal.resize(usable.columns, usable.rows);
        return usable;
      }
      throw new Error("Terminal renderer metrics were not ready within the bounded fit window");
    } finally {
      this.#finishRestore();
    }
  }

  write(bytes: Uint8Array): Promise<void> {
    return new Promise((resolve) => this.#terminal.write(bytes, resolve));
  }

  restore(snapshot: TerminalSnapshot): Promise<void> {
    this.#restorePending = true;
    if (this.#element) this.#element.style.visibility = "hidden";
    this.#terminal.reset();
    this.#terminal.resize(snapshot.columns, snapshot.rows);
    return this.write(snapshot.bytes).catch((error: unknown) => {
      this.#finishRestore();
      throw error;
    });
  }

  onData(listener: (text: string) => void): Disposable {
    return this.#terminal.onData(listener);
  }

  onResize(listener: (size: { rows: number; columns: number }) => void): Disposable {
    let active = true;
    let notificationQueued = false;
    let lastRows = this.#terminal.rows;
    let lastColumns = this.#terminal.cols;
    const subscription = this.#terminal.onResize(() => {
      if (!active || notificationQueued) return;
      notificationQueued = true;
      queueMicrotask(() => {
        notificationQueued = false;
        if (!active || this.#disposed) return;
        const rows = this.#terminal.rows;
        const columns = this.#terminal.cols;
        if (rows === lastRows && columns === lastColumns) return;
        lastRows = rows;
        lastColumns = columns;
        listener({ rows, columns });
      });
    });
    return {
      dispose: () => {
        active = false;
        subscription.dispose();
      },
    };
  }

  dispose(): void {
    this.#disposed = true;
    this.#cancelScheduledFit();
    this.#themeObserver?.disconnect();
    this.#resizeObserver?.disconnect();
    this.#terminal.dispose();
  }

  #finishRestore(): void {
    if (!this.#restorePending) return;
    this.#restorePending = false;
    if (this.#element) this.#element.style.visibility = "";
  }

  #fitIfUsable(): void {
    if (this.#disposed || !this.#element?.isConnected) return;
    const dimensions = this.#fit.proposeDimensions();
    const usable = usableDimensions(dimensions);
    if (!usable) return;
    if (usable.rows === this.#terminal.rows && usable.columns === this.#terminal.cols) return;
    this.#terminal.resize(usable.columns, usable.rows);
  }

  #scheduleFit(): void {
    this.#cancelScheduledFit();
    this.#resizeTimer = setTimeout(() => {
      this.#resizeTimer = undefined;
      this.#fitIfUsable();
    }, RESIZE_SETTLE_MS);
  }

  #cancelScheduledFit(): void {
    if (this.#resizeTimer === undefined) return;
    clearTimeout(this.#resizeTimer);
    this.#resizeTimer = undefined;
  }
}

function usableDimensions(dimensions: { rows: number; cols: number } | undefined): { rows: number; columns: number } | undefined {
  if (!dimensions || !Number.isFinite(dimensions.rows) || !Number.isFinite(dimensions.cols)) return undefined;
  const rows = Math.floor(dimensions.rows);
  const columns = Math.floor(dimensions.cols);
  if (!Number.isSafeInteger(rows) || !Number.isSafeInteger(columns) || rows < MIN_TERMINAL_ROWS || columns < MIN_TERMINAL_COLUMNS) return undefined;
  return { rows, columns };
}

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}
