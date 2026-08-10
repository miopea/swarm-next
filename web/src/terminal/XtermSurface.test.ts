import { expect, test, vi } from "vitest";

const xterm = vi.hoisted(() => ({
  fit: vi.fn(),
  terminal: undefined as { rows: number; cols: number } | undefined,
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit(): void {
      xterm.fit();
      if (xterm.terminal) {
        xterm.terminal.rows = 38;
        xterm.terminal.cols = 132;
      }
    }
  },
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;

    constructor() {
      xterm.terminal = this;
    }

    loadAddon(): void {}
    open(): void {}
    reset(): void {}
    resize(columns: number, rows: number): void {
      this.cols = columns;
      this.rows = rows;
    }
    write(_bytes: Uint8Array, callback: () => void): void {
      callback();
    }
    onData(): { dispose(): void } {
      return { dispose: vi.fn() };
    }
    onResize(): { dispose(): void } {
      return { dispose: vi.fn() };
    }
    dispose(): void {}
  },
}));

import { XtermSurface } from "./XtermSurface";

test("authoritative fit waits for two post-mount layout frames", async () => {
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));
  xterm.fit.mockClear();

  const fitting = surface.fit();
  await Promise.resolve();
  expect(xterm.fit).not.toHaveBeenCalled();

  frames.shift()?.(0);
  await Promise.resolve();
  expect(xterm.fit).not.toHaveBeenCalled();

  frames.shift()?.(16);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  expect(xterm.fit).toHaveBeenCalledTimes(1);
});
