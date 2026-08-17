import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";

import { documentColorTheme, terminalTheme } from "../brand/terminalTheme";
import type { TerminalSnapshot } from "./TerminalConnection";
import type { Disposable, TerminalSurface } from "./TerminalController";

const MAX_FIT_FRAMES = 60;
const STABLE_FIT_FRAMES = 2;
const RESIZE_SETTLE_MS = 120;
const REDRAW_RETRY_MS = 350;
const TOUCH_DRAG_THRESHOLD_PX = 4;
const FALLBACK_CELL_HEIGHT_PX = 17;
export const MIN_TERMINAL_ROWS = 4;
export const MIN_TERMINAL_COLUMNS = 20;

export class XtermSurface implements TerminalSurface {
  readonly #terminal: Terminal;
  readonly #fit = new FitAddon();
  readonly #themeObserver: MutationObserver | undefined;
  readonly #terminalResizeSubscription: Disposable;
  readonly #resizeListeners = new Map<
    (size: { rows: number; columns: number }) => void,
    { rows: number; columns: number }
  >();
  #resizeObserver: ResizeObserver | undefined;
  #resizeTimer: ReturnType<typeof setTimeout> | undefined;
  #redrawFrame: number | undefined;
  #redrawRetryTimer: ReturnType<typeof setTimeout> | undefined;
  #element: HTMLElement | undefined;
  #pointerIdentifier: number | undefined;
  #touchIdentifier: number | undefined;
  #touchLastY = 0;
  #touchDistanceY = 0;
  #touchRemainderY = 0;
  #restorePending = false;
  #geometryPublicationQueued = false;
  #geometryPublicationForced = false;
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
    this.#terminalResizeSubscription = this.#terminal.onResize(() => this.#queueGeometryPublication());
    this.#themeObserver = typeof MutationObserver === "undefined" ? undefined : new MutationObserver(() => {
      this.#terminal.options.theme = terminalTheme(documentColorTheme());
    });
    this.#themeObserver?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  }

  open(element: HTMLElement): void {
    this.#element = element;
    this.#terminal.open(element);
    // Android Chromium reports terminal drags through PointerEvent. Capture
    // that primary path at the mount boundary and retain TouchEvent only for
    // older WebKit. Registering both would apply the same physical drag twice.
    if (typeof PointerEvent === "function") {
      element.addEventListener("pointerdown", this.#handlePointerStart, { passive: true, capture: true });
      element.addEventListener("pointermove", this.#handlePointerMove, { passive: false, capture: true });
      element.addEventListener("pointerup", this.#handlePointerEnd, { passive: true, capture: true });
      element.addEventListener("pointercancel", this.#handlePointerEnd, { passive: true, capture: true });
    } else {
      element.addEventListener("touchstart", this.#handleTouchStart, { passive: true, capture: true });
      element.addEventListener("touchmove", this.#handleTouchMove, { passive: false, capture: true });
      element.addEventListener("touchend", this.#handleTouchEnd, { passive: true, capture: true });
      element.addEventListener("touchcancel", this.#handleTouchEnd, { passive: true, capture: true });
    }
    this.#resizeObserver = new ResizeObserver(() => this.#scheduleFit());
    this.#resizeObserver.observe(element);
    window.addEventListener("resize", this.#handleViewportChange);
    window.addEventListener("focus", this.#handleViewportChange);
    window.addEventListener("pageshow", this.#handleViewportChange);
    window.visualViewport?.addEventListener("resize", this.#handleViewportChange);
    document.addEventListener("visibilitychange", this.#handleVisibilityChange);
  }

  focus(): void {
    if (!this.#disposed) this.#terminal.focus();
  }

  async fit(): Promise<{ rows: number; columns: number }> {
    try {
      if (this.#disposed) throw new Error("Cannot fit a disposed terminal renderer");
      this.#cancelScheduledFit();
      await document.fonts?.ready;
      let previous: { rows: number; columns: number } | undefined;
      let stableFrames = 0;
      for (let frame = 0; frame < MAX_FIT_FRAMES; frame += 1) {
        await nextAnimationFrame();
        if (this.#disposed) throw new Error("Cannot fit a disposed terminal renderer");
        const dimensions = this.#fit.proposeDimensions();
        const usable = usableDimensions(dimensions);
        if (!usable) {
          previous = undefined;
          stableFrames = 0;
          continue;
        }
        if (previous?.rows === usable.rows && previous.columns === usable.columns) stableFrames += 1;
        else stableFrames = 1;
        previous = usable;
        if (stableFrames < STABLE_FIT_FRAMES) continue;
        this.#terminal.resize(usable.columns, usable.rows);
        this.#refreshViewport();
        return usable;
      }
      throw new Error("Terminal renderer metrics were not ready within the bounded fit window");
    } finally {
      this.#finishRestore();
    }
  }

  write(bytes: Uint8Array): Promise<void> {
    return new Promise((resolve) => this.#terminal.write(bytes, () => {
      this.#publishBufferMetrics();
      resolve();
    }));
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
    this.#resizeListeners.set(listener, { rows: this.#terminal.rows, columns: this.#terminal.cols });
    return {
      dispose: () => this.#resizeListeners.delete(listener),
    };
  }

  dispose(): void {
    this.#disposed = true;
    this.#cancelScheduledFit();
    this.#cancelScheduledRedraw();
    this.#themeObserver?.disconnect();
    this.#terminalResizeSubscription.dispose();
    this.#resizeListeners.clear();
    this.#resizeObserver?.disconnect();
    window.removeEventListener("resize", this.#handleViewportChange);
    window.removeEventListener("focus", this.#handleViewportChange);
    window.removeEventListener("pageshow", this.#handleViewportChange);
    window.visualViewport?.removeEventListener("resize", this.#handleViewportChange);
    document.removeEventListener("visibilitychange", this.#handleVisibilityChange);
    this.#element?.removeEventListener("pointerdown", this.#handlePointerStart, true);
    this.#element?.removeEventListener("pointermove", this.#handlePointerMove, true);
    this.#element?.removeEventListener("pointerup", this.#handlePointerEnd, true);
    this.#element?.removeEventListener("pointercancel", this.#handlePointerEnd, true);
    this.#element?.removeEventListener("touchstart", this.#handleTouchStart, true);
    this.#element?.removeEventListener("touchmove", this.#handleTouchMove, true);
    this.#element?.removeEventListener("touchend", this.#handleTouchEnd, true);
    this.#element?.removeEventListener("touchcancel", this.#handleTouchEnd, true);
    this.#resetTouchGesture();
    this.#terminal.dispose();
  }

  readonly #handlePointerStart = (event: PointerEvent): void => {
    if (this.#disposed || event.pointerType !== "touch" || !event.isPrimary) return;
    this.#pointerIdentifier = event.pointerId;
    this.#beginTouchGesture(event.clientY);
    try {
      this.#element?.setPointerCapture(event.pointerId);
    } catch {
      // Pointer capture is an enhancement. The capture-phase listener still
      // follows the gesture if an embedded browser declines the request.
    }
  };

  readonly #handlePointerMove = (event: PointerEvent): void => {
    if (event.pointerId !== this.#pointerIdentifier || event.pointerType !== "touch") return;
    this.#continueTouchGesture(event.clientY, event);
  };

  readonly #handlePointerEnd = (event: PointerEvent): void => {
    if (event.pointerId !== this.#pointerIdentifier) return;
    try {
      if (this.#element?.hasPointerCapture(event.pointerId)) this.#element.releasePointerCapture(event.pointerId);
    } catch {
      // The browser can release capture itself before pointercancel arrives.
    }
    this.#resetTouchGesture();
  };

  readonly #handleTouchStart = (event: TouchEvent): void => {
    if (this.#disposed || event.touches.length !== 1) {
      this.#resetTouchGesture();
      return;
    }
    const touch = event.touches.item(0);
    if (!touch) return;
    this.#touchIdentifier = touch.identifier;
    this.#beginTouchGesture(touch.clientY);
  };

  readonly #handleTouchMove = (event: TouchEvent): void => {
    if (this.#touchIdentifier === undefined || event.touches.length !== 1) {
      this.#resetTouchGesture();
      return;
    }
    const touch = Array.from(event.touches).find(({ identifier }) => identifier === this.#touchIdentifier);
    if (!touch) {
      this.#resetTouchGesture();
      return;
    }
    this.#continueTouchGesture(touch.clientY, event);
  };

  readonly #handleTouchEnd = (event: TouchEvent): void => {
    if (this.#touchIdentifier === undefined) return;
    const ended = Array.from(event.changedTouches).some(({ identifier }) => identifier === this.#touchIdentifier);
    if (ended) this.#resetTouchGesture();
  };

  readonly #handleViewportChange = (): void => {
    this.#scheduleFit();
  };

  readonly #handleVisibilityChange = (): void => {
    if (document.visibilityState === "visible") this.#scheduleFit();
  };

  #beginTouchGesture(clientY: number): void {
    this.#touchLastY = clientY;
    this.#touchDistanceY = 0;
    this.#touchRemainderY = 0;
  }

  #continueTouchGesture(clientY: number, event: TouchEvent | PointerEvent): void {
    const deltaY = this.#touchLastY - clientY;
    this.#touchLastY = clientY;
    this.#touchDistanceY += Math.abs(deltaY);
    this.#touchRemainderY += deltaY;
    if (this.#touchDistanceY < TOUCH_DRAG_THRESHOLD_PX) return;

    event.preventDefault();
    // xterm includes a document-level touch gesture recognizer. Do not let it
    // reinterpret a drag that Swarm has already translated into scrollback.
    event.stopPropagation();
    const lineHeight = this.#terminalLineHeight();
    const gestureLines = this.#touchRemainderY < 0
      ? Math.ceil(this.#touchRemainderY / lineHeight)
      : Math.floor(this.#touchRemainderY / lineHeight);
    if (gestureLines === 0) return;
    // A finger moving upward has a positive gesture delta, but xterm defines
    // negative scrollLines as moving the viewport up toward older history.
    // Keep the physical remainder in gesture coordinates and invert only at
    // the xterm boundary.
    this.#terminal.scrollLines(-gestureLines);
    this.#touchRemainderY -= gestureLines * lineHeight;
    this.#publishBufferMetrics();
  }

  #terminalLineHeight(): number {
    const height = this.#element?.clientHeight ?? 0;
    if (height > 0 && this.#terminal.rows > 0) return height / this.#terminal.rows;
    return FALLBACK_CELL_HEIGHT_PX;
  }

  #resetTouchGesture(): void {
    this.#pointerIdentifier = undefined;
    this.#touchIdentifier = undefined;
    this.#touchLastY = 0;
    this.#touchDistanceY = 0;
    this.#touchRemainderY = 0;
  }

  #finishRestore(): void {
    if (!this.#restorePending) return;
    this.#restorePending = false;
    if (this.#element) this.#element.style.visibility = "";
    // Chromium can retain a blank canvas when xterm writes a canonical
    // snapshot while its surface is hidden. Paint again on the first visible
    // frame instead of relying on the browser to invalidate the canvas.
    this.#scheduleRedraw();
  }

  #fitIfUsable(): void {
    if (this.#disposed || !this.#element?.isConnected) return;
    const dimensions = this.#fit.proposeDimensions();
    const usable = usableDimensions(dimensions);
    if (!usable) return;
    const changed = usable.rows !== this.#terminal.rows || usable.columns !== this.#terminal.cols;
    if (changed) {
      this.#terminal.resize(usable.columns, usable.rows);
    } else {
      // A different device can leave the shared PTY at stale dimensions while
      // this renderer already believes it is correctly fitted. Re-publish the
      // settled visible geometry so the active Swarm window can repair the
      // host without relying on xterm to emit a changed-size event.
      this.#queueGeometryPublication(true);
    }
    // A stable row/column count does not mean Chromium's backing canvas is
    // healthy. Explicitly repaint after responsive layout and PWA resumes.
    this.#scheduleRedraw();
  }

  #refreshViewport(): void {
    if (this.#disposed || this.#terminal.rows < 1) return;
    // Chromium can preserve a corrupt or empty GPU texture after sleep,
    // debugger attachment, or a large responsive resize. xterm documents this
    // as the recovery path for Chromium/Nvidia texture loss; a plain refresh
    // alone can leave the canvas visibly blank while its buffer is intact.
    this.#terminal.clearTextureAtlas();
    this.#terminal.refresh(0, this.#terminal.rows - 1);
  }

  #scheduleRedraw(): void {
    this.#cancelScheduledRedraw();
    this.#redrawFrame = requestAnimationFrame(() => {
      this.#redrawFrame = undefined;
      if (!this.#element?.isConnected) return;
      this.#refreshViewport();
      // Chromium can acknowledge the first invalidation while its backing
      // texture is still being recreated after a debugger/PWA viewport
      // transition. One bounded follow-up catches that race without polling.
      this.#redrawRetryTimer = setTimeout(() => {
        this.#redrawRetryTimer = undefined;
        if (this.#element?.isConnected) this.#refreshViewport();
      }, REDRAW_RETRY_MS);
    });
  }

  #cancelScheduledRedraw(): void {
    if (this.#redrawFrame !== undefined) {
      cancelAnimationFrame(this.#redrawFrame);
      this.#redrawFrame = undefined;
    }
    if (this.#redrawRetryTimer !== undefined) {
      clearTimeout(this.#redrawRetryTimer);
      this.#redrawRetryTimer = undefined;
    }
  }

  #publishBufferMetrics(): void {
    if (!this.#element) return;
    const buffer = this.#terminal.buffer.active;
    this.#element.dataset.terminalBufferLines = String(buffer.length);
    this.#element.dataset.terminalScrollbackRows = String(buffer.baseY);
    this.#element.dataset.terminalViewportRow = String(buffer.viewportY);
  }

  #queueGeometryPublication(force = false): void {
    if (this.#disposed) return;
    this.#geometryPublicationForced ||= force;
    if (this.#geometryPublicationQueued) return;
    this.#geometryPublicationQueued = true;
    queueMicrotask(() => {
      this.#geometryPublicationQueued = false;
      if (this.#disposed) return;
      const size = { rows: this.#terminal.rows, columns: this.#terminal.cols };
      const forced = this.#geometryPublicationForced;
      this.#geometryPublicationForced = false;
      for (const [listener, previous] of this.#resizeListeners) {
        if (!forced && previous.rows === size.rows && previous.columns === size.columns) continue;
        this.#resizeListeners.set(listener, size);
        listener(size);
      }
    });
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
