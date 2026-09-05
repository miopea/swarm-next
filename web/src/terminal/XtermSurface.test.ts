import { afterEach, expect, test, vi } from "vitest";

const xterm = vi.hoisted(() => ({
  fit: vi.fn(),
  propose: vi.fn<() => { rows: number; cols: number } | undefined>(),
  resizeListener: undefined as ((size: { rows: number; cols: number }) => void) | undefined,
  terminal: undefined as { rows: number; cols: number; unicode: { activeVersion: string } } | undefined,
  options: undefined as Record<string, unknown> | undefined,
  focus: vi.fn(),
  keyHandler: undefined as ((event: KeyboardEvent) => boolean) | undefined,
  breakUnicodeAddon: false,
  selection: "",
  resize: vi.fn(),
  refresh: vi.fn(),
  clearTextureAtlas: vi.fn(),
  scrollLines: vi.fn(),
  scrollToBottom: vi.fn(),
  scrollListener: undefined as ((viewportY: number) => void) | undefined,
  bufferBaseY: 0,
  bufferViewportY: 0,
  gpuMode: "unavailable" as "unavailable" | "works" | "activation-fails" | "loss-during-load",
  gpuDispose: vi.fn(),
  gpuLoss: undefined as (() => void) | undefined,
  terminalDispose: vi.fn(),
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

vi.mock("@xterm/addon-search", () => ({ SearchAddon: class {} }));
vi.mock("@xterm/addon-unicode11", () => ({
  // Throws on demand, the way an addon built for a different xterm major does.
  // That failure locked the operator out of every worker.
  Unicode11Addon: class {
    constructor() {
      if (xterm.breakUnicodeAddon) throw new Error("incompatible addon");
    }
  },
}));
vi.mock("@xterm/addon-serialize", () => ({ SerializeAddon: class {} }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
// Constructing this throws without WebGL2, which jsdom does not have — the same
// path a real browser takes on a GPU denylist. The surface must fall back.
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    readonly gpu = true;
    constructor() { if (xterm.gpuMode === "unavailable") throw new Error("WebGL2 is unavailable"); }
    onContextLoss(listener: () => void) { xterm.gpuLoss = listener; return { dispose() {} }; }
    dispose() { xterm.gpuDispose(); }
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
    // The real Terminal exposes this, and the surface activates Unicode 11 on
    // it before anything is written. A double without it would let the surface
    // stop doing that without any test noticing.
    unicode = { activeVersion: "6" };

    constructor(options: Record<string, unknown>) {
      this.options = options;
      xterm.options = options;
      xterm.terminal = this;
    }

    loadAddon(addon: { gpu?: boolean }): void {
      if (!addon.gpu) return;
      if (xterm.gpuMode === "activation-fails") throw new Error("GPU activation failed");
      if (xterm.gpuMode === "loss-during-load") xterm.gpuLoss?.();
    }
    open(): void {}
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean): void {
      xterm.keyHandler = handler;
    }
    hasSelection(): boolean { return xterm.selection.length > 0; }
    getSelection(): string { return xterm.selection; }
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
    dispose(): void { xterm.terminalDispose(); }
  },
}));

import { XtermSurface } from "./XtermSurface";

afterEach(() => {
  xterm.gpuMode = "unavailable";
  xterm.gpuLoss = undefined;
  xterm.gpuDispose.mockClear();
  xterm.terminalDispose.mockClear();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("failed GPU activation disposes the allocated addon and leaves the terminal usable", async () => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  xterm.gpuMode = "activation-fails";
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));
  expect(xterm.gpuDispose).toHaveBeenCalledTimes(1);
  await surface.write(new Uint8Array([65]));
  surface.dispose();
  surface.dispose();
  expect(xterm.gpuDispose).toHaveBeenCalledTimes(1);
  expect(xterm.terminalDispose).toHaveBeenCalledTimes(1);
});

test("a context lost during addon activation is not retained or disposed twice", () => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  xterm.gpuMode = "loss-during-load";
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));
  expect(xterm.gpuDispose).toHaveBeenCalledTimes(1);
  xterm.gpuLoss?.();
  surface.dispose();
  expect(xterm.gpuDispose).toHaveBeenCalledTimes(1);
});

function press(key: string, init: Partial<KeyboardEventInit> = {}): boolean {
  return xterm.keyHandler!(new KeyboardEvent("keydown", { key, ctrlKey: true, ...init }));
}

test("Ctrl+C copies a selection and interrupts without one", async () => {
  // Muscle memory: Ctrl+C is copy everywhere else. It is also the terminal's
  // interrupt, and losing that would be worse than the paper cut it fixes. The
  // selection decides, which is what VS Code and Windows Terminal do.
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  new XtermSurface();

  xterm.selection = "";
  expect(press("c")).toBe(true);
  expect(writeText).not.toHaveBeenCalled();

  xterm.selection = "deploy run 32667983788";
  expect(press("c")).toBe(false);
  await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith("deploy run 32667983788"));
});

test("a refused clipboard does not break the terminal", async () => {
  vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockRejectedValue(new Error("denied")) } });
  new XtermSurface();
  xterm.selection = "something";

  expect(() => press("c")).not.toThrow();
});

test("keys Claude Code binds are left to the terminal", () => {
  // Taking one of these silently removes a function the operator has: Ctrl+D is
  // EOF, Ctrl+R is reverse history search, Ctrl+L clears the screen.
  new XtermSurface();
  xterm.selection = "selected";

  for (const key of ["d", "r", "l", "a", "e", "z"]) {
    expect(press(key)).toBe(true);
  }
  // And a bare C, with no chord, is just typing.
  expect(press("c", { ctrlKey: false })).toBe(true);
});

test("Ctrl+F asks for search only once something is listening", () => {
  const surface = new XtermSurface();
  expect(press("f")).toBe(true);

  const requested = vi.fn();
  surface.onFindRequested(requested);
  expect(press("f")).toBe(false);
  expect(requested).toHaveBeenCalledOnce();
});

test("an addon that cannot load does not stop the terminal opening", () => {
  // The regression this exists to prevent: addons were loaded unguarded, one
  // threw against xterm 6, and the constructor threw with it — so the terminal
  // view failed to mount and every worker was unreachable behind an error
  // boundary telling the operator to refresh. Refreshing could not help.
  vi.stubGlobal("ResizeObserver", class {
    observe(): void {}
    disconnect(): void {}
  });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  xterm.breakUnicodeAddon = true;

  const surface = new XtermSurface();

  expect(() => surface.open(document.createElement("div"))).not.toThrow();
  // Reported rather than swallowed.
  expect(warn).toHaveBeenCalledWith(
    expect.stringContaining("unicode11"),
    expect.anything(),
  );
  warn.mockRestore();
  xterm.breakUnicodeAddon = false;
});

test("measures characters with Unicode 11 before anything is written", () => {
  // Claude Code's output is full of emoji and box drawing. xterm's default
  // width table is Unicode 6 and measures several of those a column narrow,
  // which shifts every character after them on the line. Activating after
  // content had been parsed would leave that content measured by the old table.
  new XtermSurface();

  expect(xterm.terminal?.unicode.activeVersion).toBe("11");
});

test("falls back to the DOM renderer when the GPU is unavailable", () => {
  // Constructing the WebGL addon throws without WebGL2 — headless environments,
  // remote sessions, GPU denylists. A slower terminal is not a broken one, so
  // opening must survive it. The mock throws, which is that path.
  vi.stubGlobal("ResizeObserver", class {
    observe(): void {}
    disconnect(): void {}
  });
  const surface = new XtermSurface();

  expect(() => surface.open(document.createElement("div"))).not.toThrow();
  expect(() => surface.dispose()).not.toThrow();
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

test("ownership lost during asynchronous fit prevents local grid mutation", async () => {
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  xterm.propose.mockReturnValue({ rows: 38, cols: 132 });
  const surface = new XtermSurface();
  surface.open(document.createElement("div"));
  let owner = true;
  surface.observeGeometryOwnership(() => owner);
  xterm.resize.mockClear();
  const fitting = surface.fit();
  await Promise.resolve();
  frames.shift()?.(0);
  await Promise.resolve();
  owner = false;
  frames.shift()?.(16);
  await expect(fitting).resolves.toEqual({ rows: 38, columns: 132 });
  expect(xterm.resize).not.toHaveBeenCalled();
  surface.dispose();
});

test.each([false, true])("fit waits only for terminal fonts and tolerates load failure: %s", async (fontFailure) => {
  const previousFonts = Object.getOwnPropertyDescriptor(document, "fonts");
  const load = vi.fn(() => fontFailure ? Promise.reject(new Error("font unavailable")) : Promise.resolve([]));
  Object.defineProperty(document, "fonts", { configurable: true, value: { ready: new Promise(() => {}), load } });
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  xterm.propose.mockReturnValue({ rows: 38, cols: 132 });
  const surface = new XtermSurface();
  try {
    surface.open(document.createElement("div"));
    const fitting = surface.fit();
    expect(load).toHaveBeenCalledWith(`14px ${xterm.options?.fontFamily}`, "W");
    for (let step = 0; step < 4; step++) await Promise.resolve();
    expect(frames).toHaveLength(1);
    frames.shift()?.(0);
    await Promise.resolve();
    frames.shift()?.(16);
    expect(await fitting).toEqual({ rows: 38, columns: 132 });
  } finally {
    surface.dispose();
    if (previousFonts) Object.defineProperty(document, "fonts", previousFonts);
    else Reflect.deleteProperty(document, "fonts");
  }
});

test.each(["font", "frame"])("disposal settles a fit waiting for %s without resuming layout", async (stage) => {
  const previousFonts = Object.getOwnPropertyDescriptor(document, "fonts");
  let releaseFont: () => void = () => {};
  const pendingFont = new Promise<never[]>((resolve) => { releaseFont = () => resolve([]); });
  Object.defineProperty(document, "fonts", { configurable: true, value: {
    load: () => stage === "font" ? pendingFont : Promise.resolve([]),
  } });
  const cancel = vi.fn();
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.push(callback); return 42; });
  vi.stubGlobal("cancelAnimationFrame", cancel);
  const surface = new XtermSurface();
  xterm.resize.mockClear();
  let outcome = "pending";
  const fitting = surface.fit().then(() => { outcome = "resolved"; }, () => { outcome = "rejected"; });
  try {
    for (let step = 0; step < 8; step++) await Promise.resolve();
    surface.dispose();
    for (let step = 0; step < 8; step++) await Promise.resolve();
    expect(outcome).toBe("rejected");
    await fitting;
    if (stage === "frame") expect(cancel).toHaveBeenCalledWith(42);
    releaseFont();
    frames.shift()?.(16);
    for (let step = 0; step < 8; step++) await Promise.resolve();
    expect(frames).toHaveLength(0);
    expect(xterm.resize).not.toHaveBeenCalled();
  } finally {
    surface.dispose();
    if (previousFonts) Object.defineProperty(document, "fonts", previousFonts);
    else Reflect.deleteProperty(document, "fonts");
  }
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

test("software keyboard geometry hold blocks observer mutation until release", async () => {
  vi.useFakeTimers();
  let notify: (() => void) | undefined;
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { notify = callback; }
    observe(): void {}
    disconnect(): void {}
  });
  const surface = new XtermSurface();
  const host = document.createElement("div");
  document.body.append(host);
  surface.open(host);
  const publish = vi.fn();
  surface.onResize(publish);
  let held = true;
  surface.observeGeometrySuspension(() => held);
  xterm.propose.mockReset().mockReturnValue({ rows: 12, cols: 40 });
  const resize = vi.spyOn(xterm.terminal!, "resize" as never);

  notify?.();
  await vi.advanceTimersByTimeAsync(200);
  expect(resize).not.toHaveBeenCalled();
  expect(publish).not.toHaveBeenCalled();

  held = false;
  notify?.();
  await vi.advanceTimersByTimeAsync(200);
  expect(resize).toHaveBeenCalledWith(40, 12);
  surface.dispose();
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

test("repairs the host once, not on every settled fit that changed nothing", async () => {
  // The repair exists because another device can leave the shared PTY at stale
  // dimensions while this renderer believes it is fitted. It was firing on
  // every settled fit, so most of what this surface sent asked for the size the
  // terminal was already at: measured on the live ledger, 57 of 85 requests in
  // two hours, 67%.
  vi.useFakeTimers();
  vi.stubGlobal("ResizeObserver", class { observe(): void {} disconnect(): void {} });
  const element = document.createElement("div");
  document.body.append(element);
  xterm.propose.mockReset().mockReturnValue({ rows: 24, cols: 80 });
  const surface = new XtermSurface();
  surface.open(element);
  const listener = vi.fn();
  surface.onResize(listener);

  window.dispatchEvent(new Event("resize"));
  await vi.advanceTimersByTimeAsync(RESIZE_SETTLE_FOR_TEST_MS + 16);
  expect(listener).toHaveBeenCalledTimes(1);

  // Nothing has moved. Saying it again tells the host nothing it was not told.
  for (let settle = 0; settle < 4; settle += 1) {
    window.dispatchEvent(new Event("resize"));
    await vi.advanceTimersByTimeAsync(RESIZE_SETTLE_FOR_TEST_MS + 16);
  }
  expect(listener, "a repeated repair is not worth a request").toHaveBeenCalledTimes(1);

  // A restore means the host wrote to us, so the next repair is owed again.
  await surface.restore({ rows: 24, columns: 80, sequence: 1, data: "" } as never);
  window.dispatchEvent(new Event("resize"));
  await vi.advanceTimersByTimeAsync(RESIZE_SETTLE_FOR_TEST_MS + 16);
  expect(listener.mock.calls.length, "a restore makes the repair owed again").toBeGreaterThan(1);

  surface.dispose();
  element.remove();
  vi.useRealTimers();
});

/**
 * A phone scrolling a terminal it does not own must not re-fit its grid.
 *
 * THE PATH THE OPERATOR WAS ACTUALLY HITTING, and the one two previous fixes
 * could not reach. The ResizeObserver lives in this class, and on a phone it is
 * woken by SCROLLING — hiding and showing the address bar resizes the
 * container. So every scroll narrowed the grid to phone width and reflowed a
 * desktop's wide content into it, while each arriving snapshot restored the
 * owner's width. "Terminal is unstable. Keeps jumping back time. I am required
 * to do redraws because it's unstable."
 *
 * The controller was taught not to mutate on its own three paths, which was
 * necessary and not sufficient: this mutation is inside the surface, below
 * where the controller can see.
 */
test("a surface that does not own the geometry measures on resize but never applies it", () => {
  let wake = (): void => {};
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { wake = callback; }
    observe(): void {}
    disconnect(): void {}
  });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  // The owner's width, which is what the snapshot put there.
  xterm.propose.mockReset().mockReturnValue({ rows: 24, cols: 45 });

  // Attached, because the fit path refuses to measure a detached element —
  // and a detached one would make this test pass for the wrong reason.
  const host = document.createElement("div");
  document.body.append(host);
  const surface = new XtermSurface();
  surface.open(host);
  surface.observeGeometryOwnership(() => false);

  const heard: { rows: number; columns: number }[] = [];
  surface.onResize((size) => heard.push({ rows: size.rows, columns: size.columns }));

  const before = { rows: xterm.terminal?.rows, cols: xterm.terminal?.cols };
  xterm.resize.mockClear();

  // The observer settles on a timer before it fits, so the wake alone proves
  // nothing — run the timer out.
  vi.useFakeTimers();
  wake();
  vi.runAllTimers();
  vi.useRealTimers();

  // NOT APPLIED: the owner's grid is left exactly as the snapshot set it.
  expect(xterm.resize).not.toHaveBeenCalled();
  expect(xterm.terminal).toMatchObject(before);
  // BUT STILL HEARD: refusing to mutate is not the same as going silent, and
  // the server decides the claim from what this device reports.
  expect(heard).toContainEqual({ rows: 24, columns: 45 });
});

/**
 * The other direction, which a careless guard would break: a device that owns
 * its geometry must still re-fit when its own viewport changes.
 */
test("a surface that owns the geometry still applies a resize", () => {
  let wake = (): void => {};
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { wake = callback; }
    observe(): void {}
    disconnect(): void {}
  });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  xterm.propose.mockReset().mockReturnValue({ rows: 31, cols: 97 });

  const host = document.createElement("div");
  document.body.append(host);
  const surface = new XtermSurface();
  surface.open(host);
  xterm.resize.mockClear();

  vi.useFakeTimers();
  wake();
  vi.runAllTimers();
  vi.useRealTimers();

  expect(xterm.resize).toHaveBeenCalledWith(97, 31);
});
