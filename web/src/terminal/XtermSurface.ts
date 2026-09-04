import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";

import { documentColorTheme, terminalTheme } from "../brand/terminalTheme";
import type { TerminalSnapshot } from "./TerminalConnection";
import type { Disposable, TerminalSurface } from "./TerminalController";

const MAX_FIT_FRAMES = 60;
const STABLE_FIT_FRAMES = 2;
const RESIZE_SETTLE_MS = 120;
/**
 * How recently a size has to have been left for returning to it to look like a
 * flip rather than a decision.
 *
 * Comfortably longer than the observer's settle delay, and far shorter than a
 * person changing their mind about a window.
 */
const OSCILLATION_WINDOW_MS = 1_000;
/**
 * How many recent sizes to remember when deciding whether a change is a cycle.
 *
 * Has to exceed the longest cycle worth damping. Four was observed in the wild;
 * eight leaves room without letting a legitimate resize get stuck behind
 * ancient history, which the time window bounds anyway.
 */
const REMEMBERED_SIZES = 8;
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
    (size: { rows: number; columns: number; origin: "viewport" | "restore" }) => void,
    { rows: number; columns: number }
  >();
  /**
   * Where the next published size came from.
   *
   * Restoring a canonical snapshot resizes the terminal, which publishes a size
   * exactly as a real viewport change does. Told apart, one of them is the
   * operator changing their own window and the other is this renderer echoing a
   * size that arrived from another device — and only the first should ask to
   * take authority over the PTY.
   */
  #geometryPublicationOrigin: "viewport" | "restore" = "viewport";
  /**
   * The size this surface last published purely to repair the host.
   *
   * The repair below fires on every settled fit that changed nothing, which is
   * most of them. Measured on the live ledger: 57 of 85 geometry requests in
   * two hours asked for the size the terminal was already at — 67%, against
   * the 46% recorded when this was filed. The repair is worth keeping; sending
   * it again and again after nothing has moved is not.
   *
   * Cleared whenever the world could have moved underneath us: a real resize,
   * or a snapshot restore, which is exactly when the host may hold a size we
   * do not.
   */
  #lastRepairPublished: { rows: number; columns: number } | undefined;
  /** The sizes this renderer recently applied, newest first, and when. */
  #appliedSizes: { rows: number; columns: number; at: number }[] = [];
  #resizeObserver: ResizeObserver | undefined;
  /**
   * Whether this device may resize its own grid.
   *
   * Defaults to true so a surface nobody told stays exactly as it was. The
   * controller sets it from the connection, because ownership is a fact about
   * the SESSION and this class can only see pixels.
   */
  #ownsGeometry: () => boolean = () => true;
  /** Software-keyboard viewport motion is presentation, not a PTY resize. */
  #geometrySuspended: () => boolean = () => false;
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
  readonly #search = new SearchAddon();
  #findRequested?: () => void;
  readonly #renderableListeners = new Set<(renderable: boolean) => void>();
  readonly #serialize = new SerializeAddon();
  /** Held so a lost GPU context can dispose it and fall back to the DOM. */
  #webgl?: WebglAddon;

  constructor() {
    this.#terminal = new Terminal({
      // Unicode 11 measurement is proposed API, and xterm refuses it unless
      // this is set: "You must set the allowProposedApi option to true to use
      // proposed API". Without it the addon threw on construction — which is
      // what locked every terminal behind an error boundary until addons were
      // made optional. Measured against a real browser rather than guessed:
      // with this set, all five addons load against xterm 6.
      allowProposedApi: true,
      cursorBlink: true,
      convertEol: false,
      fontFamily: '"Atkinson Hyperlegible Mono Variable", "Cascadia Code", "SFMono-Regular", Consolas, monospace',
      fontSize: 14,
      minimumContrastRatio: 4.5,
      scrollback: 1_000,
      theme: terminalTheme(documentColorTheme()),
    });
    this.#terminal.loadAddon(this.#fit);
    // Ctrl+C is muscle memory for copy and the terminal's own interrupt. Which
    // one a keypress means is decided by whether anything is selected — the
    // same rule VS Code and Windows Terminal use — so copy never costs the
    // ability to interrupt, and interrupting never costs a copy.
    this.#terminal.attachCustomKeyEventHandler((event) => this.#allowKeyEvent(event));
    // EVERY addon is optional, and none of them may take the terminal with it.
    //
    // Learned the hard way: these were loaded unguarded, one threw against
    // xterm 6, and `new XtermSurface()` threw with it — so the terminal view
    // failed to mount and the operator was locked out of every worker behind an
    // error boundary that told them to refresh. Refreshing could not help,
    // because the bundle was not stale; it was broken. A terminal without
    // clickable links is a small loss. A terminal that will not open is total.
    this.#optional("search", () => this.#terminal.loadAddon(this.#search));
    this.#optional("serialize", () => this.#terminal.loadAddon(this.#serialize));
    // Links a worker prints — PR URLs, deploy runs, health endpoints — become
    // clickable instead of something to select and retype.
    this.#optional("web-links", () => this.#terminal.loadAddon(new WebLinksAddon()));
    // Loaded AND activated before anything is written. Claude Code's own output
    // is full of emoji and box drawing, and xterm's default width table is
    // Unicode 6: it measures several of those a column narrow, which shifts
    // every character after them on the line. Activating later would leave
    // whatever had already been parsed measured by the old table.
    this.#optional("unicode11", () => {
      this.#terminal.loadAddon(new Unicode11Addon());
      this.#terminal.unicode.activeVersion = "11";
    });
    this.#terminalResizeSubscription = this.#terminal.onResize(() => this.#queueGeometryPublication());
    this.#themeObserver = typeof MutationObserver === "undefined" ? undefined : new MutationObserver(() => {
      this.#terminal.options.theme = terminalTheme(documentColorTheme());
    });
    this.#themeObserver?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  }

  /**
   * Loads one addon, or carries on without it.
   *
   * An addon is an enhancement. The terminal is the product, and no
   * enhancement may be able to stop it opening — including by throwing during
   * construction, which is how a version mismatch presents.
   */
  #optional(name: string, load: () => void): void {
    try {
      load();
    } catch (error) {
      // Reported rather than swallowed: a silently missing addon is how
      // somebody spends an afternoon wondering why search does nothing.
      console.warn(`terminal addon '${name}' is unavailable and was skipped`, error);
    }
  }

  /**
   * Draws on the GPU where the browser allows it, and silently does not where
   * it does not.
   *
   * The DOM renderer is the slowest of xterm's three, and terminal output here
   * arrives in bursts — a worker's build log is thousands of lines in a second.
   * That cost was visible: when a returning terminal drained a backlog, the
   * drain was slow enough to read as playback.
   *
   * Every failure path falls back rather than throwing. Constructing the addon
   * throws outright without WebGL2 — headless test environments, remote
   * sessions, GPU denylists — and a context can be lost at runtime when the
   * driver resets or the tab is backgrounded too long. Losing the canvas must
   * never take the terminal with it: the DOM renderer is always there, and a
   * slower terminal is not a broken one.
   */
  #useGpuRendering(): void {
    if (this.#webgl || this.#disposed) return;
    let webgl: WebglAddon | undefined;
    try {
      webgl = new WebglAddon();
      const candidate = webgl;
      this.#webgl = candidate;
      // Registered before loading: a context lost during load must still be
      // caught rather than leaving a terminal drawing to nothing.
      candidate.onContextLoss(() => {
        if (this.#webgl !== candidate) return;
        this.#webgl = undefined;
        candidate.dispose();
      });
      this.#terminal.loadAddon(candidate);
    } catch {
      // Activation can throw after allocation; release the candidate as well
      // as falling back. Do not retain an already-lost context after load.
      if (this.#webgl === webgl) {
        this.#webgl = undefined;
        try { webgl?.dispose(); } catch { /* A broken addon must not block fallback. */ }
      }
    }
  }

  /**
   * Whether xterm should handle a key, or this surface has taken it.
   *
   * Deliberately a short list. Claude Code binds most control keys itself and
   * taking one silently removes a function the operator has: Ctrl+D is EOF,
   * Ctrl+R is reverse history search, Ctrl+L clears. Those stay with the
   * terminal. Only keys whose terminal meaning is absent or recoverable are
   * taken here.
   */
  #allowKeyEvent(event: KeyboardEvent): boolean {
    if (event.type !== "keydown") return true;
    const chord = event.ctrlKey || event.metaKey;
    if (!chord || event.altKey) return true;
    const key = event.key.toLowerCase();
    if (key === "c" && this.#terminal.hasSelection()) {
      // Only with a selection. Without one this falls through and Ctrl+C is
      // the interrupt it has always been.
      void this.#copySelection();
      return false;
    }
    if (key === "f" && this.#findRequested) {
      // Free in Claude Code, so taking it costs nothing.
      event.preventDefault();
      this.#findRequested();
      return false;
    }
    return true;
  }

  async #copySelection(): Promise<void> {
    const selection = this.#terminal.getSelection();
    if (!selection) return;
    try {
      await navigator.clipboard?.writeText(selection);
    } catch {
      // A denied or unavailable clipboard must not break the terminal. The
      // selection is still there to copy by other means.
    }
  }

  /** Called when the operator asks to search this terminal. */
  onFindRequested(listener: () => void): Disposable {
    this.#findRequested = listener;
    return { dispose: () => { this.#findRequested = undefined; } };
  }

  findNext(query: string): boolean {
    return this.#search.findNext(query);
  }

  findPrevious(query: string): boolean {
    return this.#search.findPrevious(query);
  }

  /** Everything the terminal is showing, including scrollback, as text. */
  serialize(): string {
    return this.#serialize.serialize();
  }

  open(element: HTMLElement): void {
    this.#element = element;
    this.#terminal.open(element);
    this.#useGpuRendering();
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

  /**
   * Tells this surface whether it is allowed to resize its own grid.
   *
   * THE MUTATION THIS GUARDS IS IN `#fitIfUsable`, which the ResizeObserver
   * drives — and on a phone the observer is woken by SCROLLING, because hiding
   * and showing the address bar resizes the container. So a phone viewing a
   * terminal a desktop owns re-fitted its grid to phone width on every scroll,
   * while each arriving snapshot restored the owner's width. The operator:
   * "terminal is unstable. Keeps jumping back time. I am required to do redraws
   * because it's unstable."
   *
   * The controller already refused to mutate on its own three paths. It could
   * not reach this one, which is the one the operator was actually hitting.
   */
  observeGeometryOwnership(owns: () => boolean): void {
    this.#ownsGeometry = owns;
  }

  observeGeometrySuspension(suspended: () => boolean): void {
    this.#geometrySuspended = suspended;
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
        if (this.#geometrySuspended()) {
          this.#refreshViewport();
          return { rows: this.#terminal.rows, columns: this.#terminal.cols };
        }
        // Ownership can change while fonts/layout frames are awaited. A
        // passive fit measures only; it cannot reflow the canonical screen.
        if (!this.#ownsGeometry()) return usable;
        // The same anti-flap the observer path uses, because this is the path
        // the loop actually runs through.
        //
        // The guard lived only in #fitIfUsable. Measured live on the operator's
        // Hive while it was happening: 200 requests in 7 seconds alternating
        // 66x151 and 67x151 — a two-cycle, the exact shape that guard was
        // written to stop, on a build that had it. Every one of those went
        // through fit(), which applied whatever it measured and never recorded
        // what it applied, so the memory the other path reads was always empty.
        // The size it was already at is a size it has been. Recorded on the
        // first fit rather than when the surface opens, because at open the
        // terminal still carries its constructed default and the real size
        // arrives later — seeding there remembered a number that was never on
        // screen.
        if (this.#appliedSizes.length === 0) {
          this.#rememberApplied({ rows: this.#terminal.rows, columns: this.#terminal.cols });
        }
        if (this.#wouldOscillate(usable)) {
          this.#refreshViewport();
          return { rows: this.#terminal.rows, columns: this.#terminal.cols };
        }
        this.#rememberApplied(usable);
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
    if (this.#element) {
      // Names the cause, so the cover over the terminal stops describing every
      // rebuild as a layout adjustment. A recovery from a burst of build output
      // is not a layout change, and reading that it was is what made this look
      // like it popped up at random.
      this.#element.dataset.terminalRestoring = snapshot.reason;
      this.#element.setAttribute("aria-busy", "true");
    }
    this.#terminal.reset();
    // Everything this resize publishes is an echo of a size decided elsewhere.
    this.#geometryPublicationOrigin = "restore";
    this.#lastRepairPublished = undefined;
    this.#terminal.resize(snapshot.columns, snapshot.rows);
    // The cover comes down when the bytes are on screen, not when someone
    // later re-fits. It used to be removed only by `fit`, and the controller
    // deliberately skips that re-fit when the window is unfocused — so two
    // viewers of one PTY stop arguing over its size. That left the cover up
    // until the page was reloaded. What it hides is the blank between reset
    // and rewrite, and after this write there is no blank left to hide.
    return this.write(snapshot.bytes).finally(() => this.#finishRestore());
  }

  onData(listener: (text: string) => void): Disposable {
    return this.#terminal.onData(listener);
  }

  onResize(
    listener: (size: { rows: number; columns: number; origin: "viewport" | "restore" }) => void,
  ): Disposable {
    this.#resizeListeners.set(listener, { rows: this.#terminal.rows, columns: this.#terminal.cols });
    return {
      dispose: () => this.#resizeListeners.delete(listener),
    };
  }

  onScroll(listener: (atBottom: boolean) => void): Disposable {
    const notify = () => {
      const buffer = this.#terminal.buffer.active;
      listener(buffer.viewportY >= buffer.baseY);
      this.#publishBufferMetrics();
    };
    const subscription = this.#terminal.onScroll(notify);
    notify();
    return subscription;
  }

  scrollToBottom(): void {
    if (this.#disposed) return;
    this.#terminal.scrollToBottom();
    this.#publishBufferMetrics();
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.#cancelScheduledFit();
    this.#cancelScheduledRedraw();
    this.#themeObserver?.disconnect();
    this.#terminalResizeSubscription.dispose();
    this.#resizeListeners.clear();
    this.#renderableListeners.clear();
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
    // Before the terminal: the addon holds a GPU context of its own, and
    // disposing the terminal first orphans it.
    this.#webgl?.dispose();
    this.#webgl = undefined;
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
    const visible = document.visibilityState === "visible";
    if (visible) this.#scheduleFit();
    // A hidden tab cannot render: the browser throttles or stops animation
    // frames, and xterm resolves a write only once it has drawn it. That is the
    // same condition as a detached element, reached by a different door — the
    // operator went for a run and came back to the terminal replaying, with the
    // detach fix already live.
    this.#renderableListeners.forEach((listener) => listener(visible));
  };

  /**
   * Whether this surface can currently draw.
   *
   * False while the tab is hidden. The controller combines it with whether the
   * surface is attached, because either alone is enough to stop rendering and
   * neither alone is enough to allow it.
   */
  onRenderable(listener: (renderable: boolean) => void): Disposable {
    this.#renderableListeners.add(listener);
    return { dispose: () => { this.#renderableListeners.delete(listener); } };
  }

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
    // Direct manipulation: the content follows the finger. Dragging upward
    // pulls the content up and so reveals what sits below it, which in a
    // terminal is newer output. A finger moving up gives a positive gesture
    // delta, and xterm reads positive scrollLines as moving the viewport down
    // toward newer output, so the two already agree and inverting here would
    // scroll into scrollback instead.
    this.#terminal.scrollLines(gestureLines);
    this.#touchRemainderY -= gestureLines * lineHeight;
    this.#publishBufferMetrics();
  }

  /**
   * The height of one rendered row.
   *
   * Measured from a row xterm actually drew, not from the mount divided by the
   * row count. That division assumed the terminal fills its element, which
   * stopped being true once a device that does not own the geometry keeps the
   * owner's row count instead of fitting its own viewport: the rows then occupy
   * part of the element, the estimate comes out far too large, and an ordinary
   * drag rounds to zero lines. Scrolling looked dead rather than wrong.
   */
  #terminalLineHeight(): number {
    const rendered = this.#element?.querySelector<HTMLElement>(".xterm-rows > div");
    const renderedHeight = rendered?.getBoundingClientRect().height ?? 0;
    if (renderedHeight > 0) return renderedHeight;
    const rows = this.#element?.querySelector<HTMLElement>(".xterm-rows");
    const rowsHeight = rows?.getBoundingClientRect().height ?? 0;
    if (rowsHeight > 0 && this.#terminal.rows > 0) return rowsHeight / this.#terminal.rows;
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
    if (this.#element) {
      delete this.#element.dataset.terminalRestoring;
      this.#element.removeAttribute("aria-busy");
    }
    // Chromium can retain a blank canvas when xterm writes a canonical
    // snapshot while its surface is hidden. Paint again on the first visible
    // frame instead of relying on the browser to invalidate the canvas.
    this.#scheduleRedraw();
  }

  /**
   * What this viewport WOULD fit, without changing what is on screen.
   *
   * The mutating `fit` is wrong for a device that does not own the geometry: it
   * narrows the local grid before the server has agreed, and when the server
   * refuses — which it does, because another device holds the claim — the grid
   * is left reflowing the owner's wide content at this device's width. That is
   * not a small terminal, it is a shredded one, and it is what a phone showed
   * beside a desktop mid-session.
   *
   * Asking with this instead leaves the owner's columns on screen. If the claim
   * IS granted, the resize that follows arrives as a snapshot and `restore`
   * applies its dimensions, so nothing has to apply them optimistically.
   */
  proposeFit(): { rows: number; columns: number } | undefined {
    if (this.#disposed || !this.#element?.isConnected) return undefined;
    return usableDimensions(this.#fit.proposeDimensions());
  }

  #fitIfUsable(): void {
    if (this.#disposed || !this.#element?.isConnected) return;
    if (this.#geometrySuspended()) {
      this.#scheduleRedraw();
      return;
    }
    const dimensions = this.#fit.proposeDimensions();
    const usable = usableDimensions(dimensions);
    if (!usable) return;
    const changed = usable.rows !== this.#terminal.rows || usable.columns !== this.#terminal.cols;
    if (changed && this.#wouldOscillate(usable)) {
      // Applying this would put the terminal back where it was one change ago,
      // and resizing is what woke this observer in the first place.
      //
      // Measured on the operator's Hive: 200 requests in 16 seconds on one
      // device with no second viewer, alternating 65x151 / 67x151 in a clean
      // ABABAB — the terminal visibly jumping. `fit()` has always required a
      // proposal to hold still across frames before applying it; this path,
      // which is the one the observer drives, applied a single measurement and
      // so could not tell a settled size from one half of a flip.
      return;
    }
    if (changed && !this.#ownsGeometry()) {
      // MEASURE AND SAY SO, BUT DO NOT APPLY. Another device owns the grid;
      // resizing here reflows its wide content at this width, which is a
      // shredded terminal rather than a small one. Publishing the measurement
      // is still right — it is how this device asks — and the size that
      // actually lands arrives back as a snapshot.
      this.#publishProposedGeometry(usable);
      this.#scheduleRedraw();
      return;
    }
    if (changed) {
      this.#rememberApplied(usable);
      // The size moved, so any earlier repair says nothing about where the
      // host is now.
      this.#lastRepairPublished = undefined;
      this.#terminal.resize(usable.columns, usable.rows);
    } else {
      // A different device can leave the shared PTY at stale dimensions while
      // this renderer already believes it is correctly fitted. Re-publish the
      // settled visible geometry so the active Swarm window can repair the
      // host without relying on xterm to emit a changed-size event.
      //
      // Once, though, not on every settled fit. Repeating a repair the host has
      // already been told about changes nothing and is most of this surface's
      // traffic; the memo is cleared the moment anything could have moved.
      const repeated = this.#lastRepairPublished?.rows === usable.rows
        && this.#lastRepairPublished?.columns === usable.columns;
      if (!repeated) {
        this.#lastRepairPublished = { ...usable };
        this.#queueGeometryPublication(true);
      }
    }
    // A stable row/column count does not mean Chromium's backing canvas is
    // healthy. Explicitly repaint after responsive layout and PWA resumes.
    this.#scheduleRedraw();
  }

  /**
   * Whether applying this size would return to one this renderer just left.
   *
   * Remembers several sizes, not one. The first version of this compared only
   * against the size from one change ago, which damps a two-cycle and is blind
   * to anything longer — and the operator's recording showed a FOUR-cycle:
   * 24x46, 26x46, 30x46, 32x46, two hundred requests and a hundred and two
   * size changes inside one minute, on a build that already had that guard.
   *
   * Bounded in time on purpose. A genuine return to an earlier size — the
   * operator dragging a window back — must still work; what must not is cycling
   * through sizes faster than anyone could have asked for.
   */
  #wouldOscillate(next: { rows: number; columns: number }): boolean {
    const since = Date.now() - OSCILLATION_WINDOW_MS;
    // Every remembered size, including the current one.
    //
    // Skipping the current entry looked right — proposing the size you are
    // already at is not a change — but it made the guard blind to the simplest
    // cycle of all. Going A to B and straight back to A is caught only if A is
    // still in the list, and A is exactly the entry that skip removed. That is
    // the two-cycle measured live on the operator's Hive, 66x151 to 67x151 and
    // back, a hundred times in seven seconds.
    return this.#appliedSizes.some(
      (seen) => seen.at >= since && seen.rows === next.rows && seen.columns === next.columns,
    );
  }

  #rememberApplied(size: { rows: number; columns: number }): void {
    this.#appliedSizes = [{ ...size, at: Date.now() }, ...this.#appliedSizes].slice(
      0,
      REMEMBERED_SIZES,
    );
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
      const origin = this.#geometryPublicationOrigin;
      this.#geometryPublicationForced = false;
      this.#geometryPublicationOrigin = "viewport";
      for (const [listener, previous] of this.#resizeListeners) {
        if (!forced && previous.rows === size.rows && previous.columns === size.columns) continue;
        this.#resizeListeners.set(listener, size);
        listener({ ...size, origin });
      }
    });
  }

  /**
   * Publishes a size this renderer measured but deliberately did not apply.
   *
   * `#queueGeometryPublication` reads the size off the terminal, which is
   * correct everywhere else and useless here: the terminal still holds the
   * OWNER's dimensions, so publishing them would tell the server what it
   * already believes and this device would never be heard.
   */
  #publishProposedGeometry(size: { rows: number; columns: number }): void {
    if (this.#disposed) return;
    for (const [listener, previous] of this.#resizeListeners) {
      if (previous.rows === size.rows && previous.columns === size.columns) continue;
      this.#resizeListeners.set(listener, size);
      listener({ ...size, origin: "viewport" });
    }
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
