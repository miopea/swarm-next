import { afterEach, expect, test, vi } from "vitest";

const xterm = vi.hoisted(() => ({
  fit: vi.fn(),
  propose: vi.fn<() => { rows: number; cols: number } | undefined>(),
  resizeListener: undefined as ((size: { rows: number; cols: number }) => void) | undefined,
  terminal: undefined as { rows: number; cols: number } | undefined,
  options: undefined as Record<string, unknown> | undefined,
  focus: vi.fn(),
  resize: vi.fn(),
  refresh: vi.fn(),
  clearTextureAtlas: vi.fn(),
  scrollLines: vi.fn(),
  scrollToBottom: vi.fn(),
  scrollListener: undefined as ((viewportY: number) => void) | undefined,
  bufferBaseY: 0,
  bufferViewportY: 0,
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
    buffer = { active: {
      get baseY() { return xterm.bufferBaseY; },
      get viewportY() { return xterm.bufferViewportY; },
      length: 24,
    } };
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
    refresh(start: number, end: number): void { xterm.refresh(start, end); }
    clearTextureAtlas(): void { xterm.clearTextureAtlas(); }
    scrollLines(lines: number): void { xterm.scrollLines(lines); }
    scrollToBottom(): void { xterm.scrollToBottom(); }
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
    onScroll(listener: (viewportY: number) => void): { dispose(): void } {
      xterm.scrollListener = listener;
      return { dispose: vi.fn() };
    }
    dispose(): void {}
  },
}));

import { XtermSurface } from "./XtermSurface";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

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
  await Promise.resolve();
  expect(xterm.fit).not.toHaveBeenCalled();

  frames.shift()?.(48);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  expect(xterm.terminal).toMatchObject({ rows: 38, cols: 132 });
});

test("authoritative fit ignores one usable transitional mobile measurement", async () => {
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
    .mockReturnValueOnce({ rows: 7, cols: 50 })
    .mockReturnValue({ rows: 42, cols: 168 });
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));

  const fitting = surface.fit();
  await Promise.resolve();
  frames.shift()?.(0);
  await Promise.resolve();
  expect(xterm.terminal).toMatchObject({ rows: 24, cols: 80 });

  frames.shift()?.(16);
  await Promise.resolve();
  expect(xterm.terminal).toMatchObject({ rows: 24, cols: 80 });

  frames.shift()?.(32);
  await expect(fitting).resolves.toEqual({ rows: 42, columns: 168 });
  expect(xterm.terminal).toMatchObject({ rows: 42, cols: 168 });
  surface.dispose();
});

test("turns an upward one-finger drag into older terminal scrollback", () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  Object.defineProperty(element, "clientHeight", { configurable: true, value: 408 });
  const surface = new XtermSurface();
  surface.open(element);
  xterm.scrollLines.mockClear();

  element.dispatchEvent(touchEvent("touchstart", [{ identifier: 7, clientY: 300 }]));
  const move = touchEvent("touchmove", [{ identifier: 7, clientY: 249 }]);
  element.dispatchEvent(move);

  expect(move.defaultPrevented).toBe(true);
  expect(xterm.scrollLines).toHaveBeenCalledOnce();
  expect(xterm.scrollLines).toHaveBeenCalledWith(3);
  surface.dispose();
});

test("uses the Android primary pointer path without handing the drag back to xterm", () => {
  vi.stubGlobal("PointerEvent", class extends Event {});
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  const xtermChild = document.createElement("div");
  const downstreamMove = vi.fn();
  element.append(xtermChild);
  xtermChild.addEventListener("pointermove", downstreamMove);
  Object.defineProperties(element, {
    clientHeight: { configurable: true, value: 408 },
    setPointerCapture: { configurable: true, value: vi.fn() },
    hasPointerCapture: { configurable: true, value: vi.fn(() => true) },
    releasePointerCapture: { configurable: true, value: vi.fn() },
  });
  const surface = new XtermSurface();
  surface.open(element);
  xterm.scrollLines.mockClear();

  xtermChild.dispatchEvent(pointerEvent("pointerdown", { pointerId: 12, pointerType: "touch", isPrimary: true, clientY: 300 }));
  const move = pointerEvent("pointermove", { pointerId: 12, pointerType: "touch", isPrimary: true, clientY: 249 });
  xtermChild.dispatchEvent(move);

  expect(move.defaultPrevented).toBe(true);
  expect(downstreamMove).not.toHaveBeenCalled();
  expect(xterm.scrollLines).toHaveBeenCalledOnce();
  expect(xterm.scrollLines).toHaveBeenCalledWith(3);
  surface.dispose();
});

test("captures a real xterm-child drag even when the child stops bubbling", () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  const xtermChild = document.createElement("div");
  element.append(xtermChild);
  xtermChild.addEventListener("touchstart", (event) => event.stopPropagation());
  xtermChild.addEventListener("touchmove", (event) => event.stopPropagation());
  const surface = new XtermSurface();
  surface.open(element);
  xterm.scrollLines.mockClear();

  xtermChild.dispatchEvent(touchEvent("touchstart", [{ identifier: 9, clientY: 120 }]));
  const move = touchEvent("touchmove", [{ identifier: 9, clientY: 80 }]);
  xtermChild.dispatchEvent(move);

  expect(move.defaultPrevented).toBe(true);
  expect(xterm.scrollLines).toHaveBeenCalledWith(2);
  surface.dispose();
});

test("accumulates small touch movement and scrolls in both directions", () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  const surface = new XtermSurface();
  surface.open(element);
  xterm.scrollLines.mockClear();

  element.dispatchEvent(touchEvent("touchstart", [{ identifier: 3, clientY: 100 }]));
  element.dispatchEvent(touchEvent("touchmove", [{ identifier: 3, clientY: 91 }]));
  expect(xterm.scrollLines).not.toHaveBeenCalled();
  element.dispatchEvent(touchEvent("touchmove", [{ identifier: 3, clientY: 82 }]));
  expect(xterm.scrollLines).toHaveBeenLastCalledWith(1);
  element.dispatchEvent(touchEvent("touchmove", [{ identifier: 3, clientY: 108 }]));
  expect(xterm.scrollLines).toHaveBeenLastCalledWith(-1);
  surface.dispose();
});

test("drags the terminal content with the finger, not against it", () => {
  // This sign has regressed before, and a bare number in an assertion does not
  // say which way the screen moved. Stated as direct manipulation: dragging up
  // pulls content up and reveals newer output below; dragging down pulls
  // content down and reveals older scrollback above.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  const surface = new XtermSurface();
  surface.open(element);

  const drag = (from: number, to: number) => {
    xterm.scrollLines.mockClear();
    element.dispatchEvent(touchEvent("touchstart", [{ identifier: 21, clientY: from }]));
    element.dispatchEvent(touchEvent("touchmove", [{ identifier: 21, clientY: to }]));
    element.dispatchEvent(touchEvent("touchend", [{ identifier: 21, clientY: to }]));
    return xterm.scrollLines.mock.calls.at(-1)?.[0] as number | undefined;
  };

  const draggingUp = drag(300, 200);
  const draggingDown = drag(200, 300);

  // xterm reads positive scrollLines as moving toward newer output.
  expect(draggingUp).toBeGreaterThan(0);
  expect(draggingDown).toBeLessThan(0);
  surface.dispose();
});

test("ignores multi-touch gestures and removes touch handling on disposal", () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  const surface = new XtermSurface();
  surface.open(element);
  xterm.scrollLines.mockClear();

  element.dispatchEvent(touchEvent("touchstart", [
    { identifier: 1, clientY: 200 },
    { identifier: 2, clientY: 220 },
  ]));
  element.dispatchEvent(touchEvent("touchmove", [{ identifier: 1, clientY: 100 }]));
  expect(xterm.scrollLines).not.toHaveBeenCalled();

  surface.dispose();
  element.dispatchEvent(touchEvent("touchstart", [{ identifier: 4, clientY: 200 }]));
  element.dispatchEvent(touchEvent("touchmove", [{ identifier: 4, clientY: 100 }]));
  expect(xterm.scrollLines).not.toHaveBeenCalled();
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
  await Promise.resolve();
  expect(xterm.terminal).toMatchObject({ rows: 24, cols: 80 });

  frames.shift()?.(32);
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
  xterm.refresh.mockClear();
  xterm.clearTextureAtlas.mockClear();
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
  await vi.advanceTimersByTimeAsync(16);
  expect(xterm.refresh).toHaveBeenCalledWith(0, 37);

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
  expect(listener).toHaveBeenCalledWith({ rows: 41, columns: 154, origin: "viewport" });
});

/**
 * The cover names its cause while it is up, and comes down when the bytes are
 * on screen. It used to stay up until a refit, which is what left it stuck on
 * the operator's terminal until they reloaded the page.
 */
test("a covered rebuild names its cause, uncovers on write, and refits after", async () => {
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
  document.body.append(element);
  surface.open(element);
  xterm.bufferBaseY = 48;

  const restoring = surface.restore({
    sequence: 1,
    rows: 24,
    columns: 80,
    truncated: false,
    reason: "fell_behind" as const,
    bytes: new TextEncoder().encode("snapshot"),
  });
  // Carries the cause while it is covered, so the cover names what happened
  // rather than calling every rebuild a layout adjustment.
  expect(element.dataset.terminalRestoring).toBe("fell_behind");
  expect(element).toHaveAttribute("aria-busy", "true");

  await restoring;
  // Down as soon as the screen is correct — not held for a refit that may
  // never come, which is what left it covered until a page reload.
  expect(element.dataset.terminalRestoring).toBeUndefined();
  expect(element).not.toHaveAttribute("aria-busy");
  expect(element.dataset.terminalScrollbackRows).toBe("48");
  expect(element.dataset.terminalBufferLines).toBe("24");

  // Chromium can hold a blank canvas after a snapshot is written to a hidden
  // surface, so uncovering schedules a repaint. It runs with the rebuild now
  // rather than riding on a later fit.
  xterm.refresh.mockClear();
  frames.shift()?.(0);
  expect(xterm.refresh).toHaveBeenCalledWith(0, 23);

  const fitting = surface.fit();
  await Promise.resolve();
  frames.shift()?.(0);
  await Promise.resolve();
  frames.shift()?.(16);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  surface.dispose();
  element.remove();
});

test("repaints unchanged geometry after a settled viewport change", async () => {
  vi.useFakeTimers();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const element = document.createElement("div");
  document.body.append(element);
  xterm.propose.mockReset().mockReturnValue({ rows: 24, cols: 80 });
  xterm.resize.mockClear();
  xterm.refresh.mockClear();
  xterm.clearTextureAtlas.mockClear();
  const surface = new XtermSurface();
  surface.open(element);
  const listener = vi.fn();
  surface.onResize(listener);

  window.dispatchEvent(new Event("resize"));
  await vi.advanceTimersByTimeAsync(RESIZE_SETTLE_FOR_TEST_MS + 16);

  expect(xterm.resize).not.toHaveBeenCalled();
  expect(listener).toHaveBeenCalledWith({ rows: 24, columns: 80, origin: "viewport" });
  expect(xterm.clearTextureAtlas).toHaveBeenCalledOnce();
  expect(xterm.refresh).toHaveBeenCalledWith(0, 23);
  await vi.advanceTimersByTimeAsync(350);
  expect(xterm.clearTextureAtlas).toHaveBeenCalledTimes(2);
  xterm.clearTextureAtlas.mockClear();
  window.dispatchEvent(new Event("focus"));
  await vi.advanceTimersByTimeAsync(RESIZE_SETTLE_FOR_TEST_MS + 16);
  expect(xterm.clearTextureAtlas).toHaveBeenCalledOnce();
  surface.dispose();
  element.remove();
});

const RESIZE_SETTLE_FOR_TEST_MS = 120;

function touchEvent(
  type: "touchstart" | "touchmove" | "touchend" | "touchcancel",
  touches: Array<Pick<Touch, "identifier" | "clientY">>,
): TouchEvent {
  const event = new Event(type, { bubbles: true, cancelable: true }) as TouchEvent;
  const touchList = Object.assign([...touches], {
    item: (index: number) => touches[index] ?? null,
  }) as unknown as TouchList;
  Object.defineProperties(event, {
    touches: { value: type === "touchend" || type === "touchcancel" ? Object.assign([], { item: () => null }) : touchList },
    changedTouches: { value: touchList },
  });
  return event;
}

function pointerEvent(
  type: "pointerdown" | "pointermove" | "pointerup" | "pointercancel",
  init: Pick<PointerEvent, "pointerId" | "pointerType" | "isPrimary" | "clientY">,
): PointerEvent {
  const event = new Event(type, { bubbles: true, cancelable: true }) as PointerEvent;
  Object.defineProperties(event, {
    pointerId: { value: init.pointerId },
    pointerType: { value: init.pointerType },
    isPrimary: { value: init.isPrimary },
    clientY: { value: init.clientY },
  });
  return event;
}

test("a rebuilt screen uncovers itself without waiting for a later fit", async () => {
  // Reported as: the cover appeared and stayed until a refresh.
  //
  // It was only ever removed by fit()'s finally block, so a restore that is
  // not followed by a fit left it up permanently. That path is reachable and
  // deliberate: the controller skips the re-fit when the window is not focused,
  // so that two viewers of one PTY do not argue over its size forever. The
  // cover-removal went with it silently.
  //
  // Once the snapshot bytes are written the screen is correct, which is the
  // moment the cover has to come down — whether or not anyone re-fits later.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      disconnect(): void {}
    },
  );
  const surface = new XtermSurface();
  const element = document.createElement("div");
  document.body.append(element);
  surface.open(element);

  await surface.restore({
    sequence: 1,
    rows: 24,
    columns: 80,
    truncated: false,
    reason: "attached" as const,
    bytes: new TextEncoder().encode("snapshot"),
  });

  expect(element.dataset.terminalRestoring).toBeUndefined();
  expect(element).not.toHaveAttribute("aria-busy");
  surface.dispose();
  element.remove();
});

test("scrolls by a real row height rather than one guessed from the mount", async () => {
  // "I still cannot scroll" — and a reload did not fix it.
  //
  // The row height was the mount's height divided by the row count, which is
  // only correct while the terminal fills its element. A device that does not
  // own the geometry now keeps the owner's row count instead of fitting its own
  // viewport, so the rows occupy part of a taller element, the estimate comes
  // out far too large, and an ordinary drag rounds to zero lines.
  vi.stubGlobal("ResizeObserver", class { observe(): void {} disconnect(): void {} });
  const surface = new XtermSurface();
  const element = document.createElement("div");
  // A tall mount holding a short terminal: the case that broke.
  Object.defineProperty(element, "clientHeight", { value: 800, configurable: true });
  document.body.append(element);
  surface.open(element);

  // xterm's rendered rows, which are what the reading should come from.
  const rows = document.createElement("div");
  rows.className = "xterm-rows";
  const row = document.createElement("div");
  Object.defineProperty(row, "getBoundingClientRect", {
    value: () => ({ height: 17, width: 100, top: 0, bottom: 17, left: 0, right: 100 }),
  });
  rows.append(row);
  element.append(rows);

  xterm.scrollLines.mockClear();
  const touch = (type: "touchstart" | "touchmove", clientY: number) =>
    element.dispatchEvent(touchEvent(type, [{ identifier: 1, clientY }]));

  touch("touchstart", 400);
  touch("touchmove", 360);

  // Forty pixels against a real seventeen-pixel row is two lines. Against the
  // guessed height it was zero, and nothing moved.
  expect(xterm.scrollLines).toHaveBeenCalled();
  surface.dispose();
  element.remove();
});

/**
 * Measured on the operator's Hive: one device, no second viewer, 200 size
 * requests in 16 seconds alternating 65x151 / 67x151 in a clean ABABAB. The
 * terminal visibly jumped.
 *
 * The observer-driven path applied whatever a single measurement said, and
 * applying it resized the DOM, which woke the observer again. `fit()` has
 * always required a proposal to hold still across frames before applying it;
 * this path had no such check and so could not tell a settled size from one
 * half of a flip.
 */
test("stops resizing when the new size would undo the last one", async () => {
  vi.useFakeTimers();
  let notify: (() => void) | undefined;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) { notify = callback; }
      observe(): void {}
      disconnect(): void {}
    },
  );
  const surface = new XtermSurface();
  // Connected on purpose: the observer-driven fit refuses to run on a detached
  // element, so a bare createElement made this test pass without the guard.
  const host = document.createElement("div");
  document.body.append(host);
  surface.open(host);
  if (xterm.terminal) {
    xterm.terminal.rows = 65;
    xterm.terminal.cols = 151;
  }

  // The container measures two rows taller, then two rows shorter, then taller
  // again — the loop, exactly as recorded.
  const sizes = [
    { rows: 67, cols: 151 },
    { rows: 65, cols: 151 },
    { rows: 67, cols: 151 },
    { rows: 65, cols: 151 },
  ];
  let applied = 0;
  xterm.propose.mockReset().mockImplementation(() => sizes[Math.min(applied, sizes.length - 1)]);
  const resize = vi.spyOn(xterm.terminal!, "resize" as never);

  for (const _ of sizes) {
    notify?.();
    await vi.advanceTimersByTimeAsync(200);
    applied += 1;
  }

  // The first change is real and applied. Bouncing back is not.
  expect(resize.mock.calls.length).toBeLessThanOrEqual(2);
  host.remove();
  vi.useRealTimers();
});

/**
 * The operator's screen recording, read back against the geometry ledger:
 * 200 size requests in one minute, 102 size changes, cycling FOUR sizes —
 * 24x46, 26x46, 30x46, 32x46 — on a build that already carried the first
 * version of this guard.
 *
 * That guard compared only against the size from one change ago, so it damps a
 * two-cycle and is blind to anything longer. This is the case it missed.
 */
test("stops a cycle longer than two sizes", async () => {
  vi.useFakeTimers();
  let notify: (() => void) | undefined;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) { notify = callback; }
      observe(): void {}
      disconnect(): void {}
    },
  );
  const surface = new XtermSurface();
  const host = document.createElement("div");
  document.body.append(host);
  surface.open(host);
  if (xterm.terminal) {
    xterm.terminal.rows = 24;
    xterm.terminal.cols = 46;
  }

  // The four-cycle exactly as recorded, run round twice.
  const cycle = [
    { rows: 26, cols: 46 },
    { rows: 30, cols: 46 },
    { rows: 32, cols: 46 },
    { rows: 24, cols: 46 },
  ];
  let step = 0;
  xterm.propose.mockReset().mockImplementation(() => cycle[step % cycle.length]);
  const resize = vi.spyOn(xterm.terminal!, "resize" as never);

  for (let round = 0; round < 8; round += 1) {
    notify?.();
    await vi.advanceTimersByTimeAsync(200);
    step += 1;
  }

  // The first pass through unseen sizes is legitimate; going round again is not.
  expect(resize.mock.calls.length).toBeLessThanOrEqual(4);
  host.remove();
  vi.useRealTimers();
});

/**
 * Measured on the operator's Hive while it was happening: 200 size requests in
 * 7 seconds alternating 66x151 and 67x151 — a two-cycle, the exact shape the
 * oscillation guard was written to stop, on a build that already had it.
 *
 * Every one of those went through fit(), which had no guard and, worse, never
 * recorded what it applied — so the memory the observer path reads was always
 * empty and its guard could never fire either.
 */
test("fit refuses to apply a size it just left", async () => {
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("ResizeObserver", class { observe(): void {} disconnect(): void {} });
  const surface = new XtermSurface();
  const host = document.createElement("div");
  document.body.append(host);
  surface.open(host);
  if (xterm.terminal) {
    xterm.terminal.rows = 66;
    xterm.terminal.cols = 151;
  }

  const sizes = [
    { rows: 67, cols: 151 },
    { rows: 66, cols: 151 },
    { rows: 67, cols: 151 },
  ];
  let step = 0;
  xterm.propose.mockReset().mockImplementation(() => sizes[Math.min(step, sizes.length - 1)]);
  const resize = vi.spyOn(xterm.terminal!, "resize" as never);

  for (const _ of sizes) {
    const fitting = surface.fit();
    for (let frame = 0; frame < 8; frame += 1) {
      await Promise.resolve();
      frames.shift()?.(frame * 16);
      await Promise.resolve();
    }
    await fitting;
    step += 1;
  }

  // 66 -> 67 is real. Going back to 66, then to 67 again, is the loop.
  expect(resize.mock.calls.length).toBeLessThanOrEqual(1);
  host.remove();
});
