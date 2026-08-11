import { expect, test, vi } from "vitest";

const xterm = vi.hoisted(() => ({
  fit: vi.fn(),
  propose: vi.fn<() => { rows: number; cols: number } | undefined>(),
  resizeListener: undefined as ((size: { rows: number; cols: number }) => void) | undefined,
  terminal: undefined as { rows: number; cols: number } | undefined,
  options: undefined as Record<string, unknown> | undefined,
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit(): void {
      xterm.fit();
      const dimensions = xterm.propose();
      if (xterm.terminal && dimensions) {
        xterm.terminal.rows = dimensions.rows;
        xterm.terminal.cols = dimensions.cols;
      }
    }

    proposeDimensions(): { rows: number; cols: number } | undefined {
      return xterm.propose();
    }
  },
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    rows = 24;
    cols = 80;
    options: Record<string, unknown>;

    constructor(options: Record<string, unknown>) {
      this.options = options;
      xterm.options = options;
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
    onResize(listener: (size: { rows: number; cols: number }) => void): { dispose(): void } {
      xterm.resizeListener = listener;
      return { dispose: vi.fn() };
    }
    dispose(): void {}
  },
}));

import { XtermSurface } from "./XtermSurface";

test("uses the complete botanical ANSI palette", () => {
  document.documentElement.dataset.theme = "dark";
  const surface = new XtermSurface();
  const theme = xterm.options?.theme as Record<string, string>;

  expect(xterm.options?.minimumContrastRatio).toBe(4.5);
  expect(theme).toMatchObject({
    background: "#091110",
    foreground: "#f2ead8",
    red: "#d98b86",
    green: "#9eb68b",
    yellow: "#d9ad58",
    blue: "#87afc4",
    magenta: "#b7a0c8",
    cyan: "#82b8ae",
    brightRed: "#f0a09a",
    brightGreen: "#bad19f",
  });
  surface.dispose();
  delete document.documentElement.dataset.theme;
});

test("updates the palette in place when the application theme changes", async () => {
  document.documentElement.dataset.theme = "light";
  const surface = new XtermSurface();
  const lightTheme = xterm.options?.theme as Record<string, string>;
  expect(lightTheme.background).toBe("#111a18");

  document.documentElement.dataset.theme = "dark";
  await new Promise((resolve) => setTimeout(resolve, 0));

  const darkTheme = xterm.options?.theme as Record<string, string>;
  expect(darkTheme.background).toBe("#091110");
  expect(darkTheme.cursor).toBe("#e7b74e");
  expect(xterm.options?.theme).not.toBe(lightTheme);

  surface.dispose();
  delete document.documentElement.dataset.theme;
});

test("authoritative fit waits until xterm can propose real dimensions", async () => {
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
  xterm.propose
    .mockReset()
    .mockReturnValueOnce(undefined)
    .mockReturnValueOnce(undefined)
    .mockReturnValue({ rows: 38, cols: 132 });
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
  expect(xterm.terminal).toMatchObject({ rows: 38, cols: 132 });
});

test("stale queued resize events cannot replace the fitted geometry", () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));
  if (xterm.terminal) {
    xterm.terminal.rows = 38;
    xterm.terminal.cols = 132;
  }
  const listener = vi.fn();
  surface.onResize(listener);

  xterm.resizeListener?.({ rows: 24, cols: 80 });
  expect(listener).not.toHaveBeenCalled();

  xterm.resizeListener?.({ rows: 38, cols: 132 });
  expect(listener).toHaveBeenCalledWith({ rows: 38, columns: 132 });
});

test("snapshot geometry stays hidden until the visible renderer is refitted", async () => {
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
  xterm.propose.mockReset().mockReturnValue({ rows: 38, cols: 132 });
  const surface = new XtermSurface();
  const element = document.createElement("div");
  surface.open(element);

  await surface.restore({
    sequence: 1,
    rows: 24,
    columns: 80,
    truncated: false,
    bytes: new TextEncoder().encode("snapshot"),
  });
  expect(element.style.visibility).toBe("hidden");

  const fitting = surface.fit();
  await Promise.resolve();
  frames.shift()?.(0);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  expect(element.style.visibility).toBe("");
});
