import { afterEach, expect, test, vi } from "vitest";

const xterm = vi.hoisted(() => ({
  fit: vi.fn(),
  propose: vi.fn<() => { rows: number; cols: number } | undefined>(),
  resizeListener: undefined as ((size: { rows: number; cols: number }) => void) | undefined,
  terminal: undefined as { rows: number; cols: number } | undefined,
  options: undefined as Record<string, unknown> | undefined,
  focus: vi.fn(),
  resize: vi.fn(),
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
    focus(): void { xterm.focus(); }
    reset(): void {}
    resize(columns: number, rows: number): void {
      xterm.resize(columns, rows);
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

afterEach(() => vi.useRealTimers());

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

test("delegates keyboard focus to xterm's input surface", () => {
  const surface = new XtermSurface();
  surface.focus();
  expect(xterm.focus).toHaveBeenCalledOnce();
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
    .mockReturnValueOnce({ rows: 1, cols: 1 })
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
  await Promise.resolve();
  expect(xterm.fit).not.toHaveBeenCalled();

  frames.shift()?.(32);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  expect(xterm.terminal).toMatchObject({ rows: 38, cols: 132 });
});

test("authoritative fit rejects non-finite geometry and normalizes fractional measurements", async () => {
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
    .mockReturnValueOnce({ rows: Number.NaN, cols: Number.POSITIVE_INFINITY })
    .mockReturnValue({ rows: 38.9, cols: 132.7 });
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));

  const fitting = surface.fit();
  await Promise.resolve();
  frames.shift()?.(0);
  await Promise.resolve();
  expect(xterm.terminal).toMatchObject({ rows: 24, cols: 80 });

  frames.shift()?.(16);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  expect(xterm.terminal).toMatchObject({ rows: 38, cols: 132 });
  surface.dispose();
});

test("hidden or collapsing layouts cannot resize the canonical terminal", () => {
  let observed: (() => void) | undefined;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) { observed = callback; }
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  document.body.append(element);
  xterm.propose.mockReset().mockReturnValue({ rows: 1, cols: 1 });
  const surface = new XtermSurface();
  surface.open(element);
  const before = { rows: xterm.terminal?.rows, cols: xterm.terminal?.cols };

  observed?.();

  expect(xterm.terminal).toMatchObject(before);
  surface.dispose();
  element.remove();
});

test("continuous container movement produces one settled terminal resize", async () => {
  vi.useFakeTimers();
  let observed: (() => void) | undefined;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) { observed = callback; }
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  document.body.append(element);
  xterm.propose.mockReset().mockReturnValue({ rows: 38, cols: 132 });
  xterm.resize.mockClear();
  const surface = new XtermSurface();
  surface.open(element);

  observed?.();
  await vi.advanceTimersByTimeAsync(80);
  observed?.();
  await vi.advanceTimersByTimeAsync(119);
  expect(xterm.resize).not.toHaveBeenCalled();

  await vi.advanceTimersByTimeAsync(1);
  expect(xterm.resize).toHaveBeenCalledOnce();
  expect(xterm.resize).toHaveBeenCalledWith(132, 38);

  surface.dispose();
  element.remove();
});

test("resize events publish the settled renderer geometry and coalesce stale events", async () => {
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
  await Promise.resolve();
  expect(listener).not.toHaveBeenCalled();

  xterm.resizeListener?.({ rows: 38, cols: 132 });
  if (xterm.terminal) {
    xterm.terminal.rows = 41;
    xterm.terminal.cols = 154;
  }
  xterm.resizeListener?.({ rows: 41, cols: 154 });
  await Promise.resolve();
  expect(listener).toHaveBeenCalledOnce();
  expect(listener).toHaveBeenCalledWith({ rows: 41, columns: 154 });
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
