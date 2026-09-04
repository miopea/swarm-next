import type {
  TerminalConnectionHandlers,
  TerminalConnectionState,
  TerminalSnapshot,
  TerminalControlView,
} from "./TerminalConnection";
import { TerminalRestoreEvidence } from "./TerminalRestoreEvidence";

interface ControllerLifecycle {
  attached(): void;
  inactive(): void;
  stateChanged(state: TerminalConnectionState): void;
}

export interface Disposable {
  dispose(): void;
}

export interface TerminalSurface {
  open(element: HTMLElement): void;
  focus(): void;
  fit(): Promise<{ rows: number; columns: number }>;
  /**
   * What this viewport would fit, WITHOUT applying it. Optional so a test
   * double that does not model it keeps working; a surface without it simply
   * does not ask for geometry it does not own.
   */
  proposeFit?(): { rows: number; columns: number } | undefined;
  /**
   * Lets this controller tell the surface whether it may resize its own grid.
   * Optional so a test double without it keeps working.
   */
  observeGeometryOwnership?(owns: () => boolean): void;
  write(bytes: Uint8Array): Promise<void>;
  restore(snapshot: TerminalSnapshot): Promise<void>;
  onData(listener: (text: string) => void): Disposable;
  onResize(
    listener: (size: { rows: number; columns: number; origin: "viewport" | "restore" }) => void,
  ): Disposable;
  onScroll(listener: (atBottom: boolean) => void): Disposable;
  scrollToBottom(): void;
  dispose(): void;
  /**
   * Search over the terminal and its scrollback. Optional so a test double that
   * does not model it keeps working.
   */
  onFindRequested?(listener: () => void): Disposable;
  /** Whether the surface can draw right now — false while the tab is hidden. */
  onRenderable?(listener: (renderable: boolean) => void): Disposable;
  findNext?(query: string): boolean;
  findPrevious?(query: string): boolean;
}

export interface TerminalConnectionLike {
  start(handlers: TerminalConnectionHandlers): void;
  sendInput(text: string): boolean;
  resize(rows: number, columns: number, intent?: "operator" | "echo"): void;
  dispose(): void;
  /**
   * Whether this device may set the terminal's size. Optional so a test double
   * that does not model the claim behaves as it did before — owning it.
   */
  readonly ownsGeometry?: boolean;
  readonly controlView?: TerminalControlView;
  resumeHere?(rows: number, columns: number): boolean;
  releaseControl?(): void;
  /**
   * Told when there is, and is not, a surface on screen to draw into.
   *
   * Optional so a test double that does not model it behaves as it did before.
   */
  suspendRendering?(): void;
  resumeRendering?(): void;
}

export type TerminalSurfaceFactory = () => TerminalSurface;
export type TerminalConnectionFactory = () => TerminalConnectionLike;
export type TerminalStatusListener = (state: TerminalConnectionState, detail?: string) => void;
export type TerminalScrollListener = (atBottom: boolean) => void;

const MAX_STATUS_SUBSCRIBERS = 8;

/** Owns renderer and transport lifecycles independently from React views. */
export class TerminalController {
  readonly #host = document.createElement("div");
  readonly #surface: TerminalSurface;
  readonly #connection: TerminalConnectionLike;
  /**
   * Both halves of "can anything draw this".
   *
   * Attached answers whether the host is in the document; visible answers
   * whether the tab is on screen. Either alone stops rendering and neither
   * alone allows it — the first fix keyed on attachment only, and the operator
   * came back from a run to a terminal replaying, because a backgrounded tab
   * never detaches.
   */
  #attached = false;
  #visible = true;
  readonly #surfaceSubscriptions: Disposable[];
  readonly #statusSubscribers = new Set<TerminalStatusListener>();
  readonly #scrollSubscribers = new Set<TerminalScrollListener>();
  readonly #controlSubscribers = new Set<(control: TerminalControlView) => void>();
  #opened = false;
  #started = false;
  #startPromise: Promise<void> | undefined;
  #refitPromise: Promise<void> | undefined;
  #disposed = false;
  #state: TerminalConnectionState = "connecting";
  #stateDetail: string | undefined;
  #pendingFocus: "container" | "input" | undefined;
  #focusOnConnect: "container" | "input" | undefined;
  #lastRequestedFocus: "container" | "input" | undefined;
  #atBottom = true;
  readonly #lifecycle: ControllerLifecycle | undefined;

  constructor(surfaceFactory: TerminalSurfaceFactory, connectionFactory: TerminalConnectionFactory, lifecycle?: ControllerLifecycle) {
    this.#lifecycle = lifecycle;
    this.#surface = surfaceFactory();
    this.#connection = connectionFactory();
    this.#host.className = "terminal-surface";
    this.#host.tabIndex = -1;
    this.#surfaceSubscriptions = [
      this.#surface.onData((text) => this.#connection.sendInput(text)),
      this.#surface.onScroll((atBottom) => this.#setAtBottom(atBottom)),
      this.#surface.onRenderable?.((renderable) => {
        this.#visible = renderable;
        if (!renderable) this.#lifecycle?.inactive();
        this.#updateRendering();
      }) ?? { dispose: () => undefined },
    ];
  }

  attach(container: HTMLElement): void {
    if (this.#disposed) throw new Error("Cannot attach a disposed terminal");
    container.replaceChildren(this.#host);
    this.#lifecycle?.attached();
    if (!this.#opened) {
      this.#surface.open(this.#host);
      this.#opened = true;
    }
    this.#attached = true;
    this.#updateRendering();
    if (this.#started) {
      this.#applyPendingFocus();
      this.#refitWhenAttached();
    } else {
      this.#startWhenFitted();
    }
  }

  detach(): void {
    // The surface stays open — only its host leaves the document. xterm cannot
    // render into a detached element, so frames arriving now would queue behind
    // a write that cannot finish and replay on return.
    this.#attached = false;
    this.#lifecycle?.inactive();
    this.#connection.releaseControl?.();
    this.#updateRendering();
    this.#host.remove();
  }

  get attached(): boolean { return this.#attached; }

  /** Told when the operator asks to search this terminal. */
  #updateRendering(): void {
    if (this.#attached && this.#visible) this.#connection.resumeRendering?.();
    else this.#connection.suspendRendering?.();
  }

  subscribeFind(listener: () => void): Disposable {
    return this.#surface.onFindRequested?.(listener) ?? { dispose: () => undefined };
  }

  find(query: string, direction: "next" | "previous"): boolean {
    const search = direction === "next" ? this.#surface.findNext : this.#surface.findPrevious;
    return search?.call(this.#surface, query) ?? false;
  }

  subscribe(listener: TerminalStatusListener): Disposable {
    if (this.#statusSubscribers.size >= MAX_STATUS_SUBSCRIBERS) {
      throw new Error(`Terminal status subscriber limit of ${MAX_STATUS_SUBSCRIBERS} reached`);
    }
    this.#statusSubscribers.add(listener);
    listener(this.#state, this.#stateDetail);
    return { dispose: () => this.#statusSubscribers.delete(listener) };
  }

  sendInput(text: string): boolean {
    if (this.#disposed) throw new Error("Cannot send input to a disposed terminal");
    return this.#connection.sendInput(text);
  }

  subscribeControl(listener: (control: TerminalControlView) => void): Disposable {
    if (this.#controlSubscribers.size >= MAX_STATUS_SUBSCRIBERS) throw new Error("Terminal control subscriber limit reached");
    this.#controlSubscribers.add(listener);
    listener(this.#connection.controlView ?? "owned");
    return { dispose: () => this.#controlSubscribers.delete(listener) };
  }

  resumeHere(): boolean {
    if (this.#disposed || !this.#attached || !this.#visible) return false;
    const size = this.#surface.proposeFit?.();
    return size ? this.#connection.resumeHere?.(size.rows, size.columns) ?? false : false;
  }

  requestFocus(input: boolean): void {
    if (this.#disposed) return;
    this.#pendingFocus = input ? "input" : "container";
    this.#lastRequestedFocus = this.#pendingFocus;
    this.#focusOnConnect = this.#state === "connected" ? undefined : this.#pendingFocus;
    this.#applyPendingFocus();
  }

  subscribeScroll(listener: TerminalScrollListener): Disposable {
    this.#scrollSubscribers.add(listener);
    listener(this.#atBottom);
    return { dispose: () => this.#scrollSubscribers.delete(listener) };
  }

  scrollToBottom(): void {
    if (this.#disposed) return;
    this.#surface.scrollToBottom();
  }

  /** Refit and repaint a connected renderer without replacing its transport. */
  async redraw(): Promise<void> {
    if (this.#disposed || !this.#opened || !this.#host.parentElement) return;
    await this.#refitAttachedSurface();
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.detach();
    for (const subscription of this.#surfaceSubscriptions) subscription.dispose();
    this.#connection.dispose();
    this.#surface.dispose();
    this.#statusSubscribers.clear();
    this.#scrollSubscribers.clear();
    this.#controlSubscribers.clear();
  }

  #startWhenFitted(): void {
    if (this.#started || this.#startPromise) return;
    const startPromise = this.#fitAndStart();
    this.#startPromise = startPromise;
    void startPromise
      .catch((error: unknown) => {
        if (!this.#disposed) {
          this.#setState(
            "error",
            error instanceof Error ? error.message : "terminal renderer fit failed",
          );
        }
      })
      .finally(() => {
        if (this.#startPromise === startPromise) this.#startPromise = undefined;
      });
  }

  #refitWhenAttached(): void {
    if (this.#refitPromise) return;
    const refitPromise = this.#refitAttachedSurface("echo");
    this.#refitPromise = refitPromise;
    void refitPromise
      // A started terminal already owns a valid PTY size. Reattachment can race
      // a hidden or not-yet-laid-out container; ResizeObserver will publish the
      // settled geometry later, so a transient refit miss must not poison the
      // otherwise healthy transport.
      .catch(() => undefined)
      .finally(() => {
        if (this.#refitPromise === refitPromise) this.#refitPromise = undefined;
      });
  }

  /**
   * Measures this viewport, applying it locally ONLY if this device owns the
   * geometry.
   *
   * THE INVARIANT, in one place: a device that does not own the geometry never
   * mutates its own grid. `fit()` resizes the local grid as a side effect, so
   * calling it while another device holds the claim reflows that device's wide
   * content at this width — a shredded terminal, not a small one.
   *
   * This exists because stating the rule at each call site did not survive.
   * aa4de4a fixed one direction by calling `fit()` where it should not have;
   * e088777 fixed the snapshot path and left the other two, so a phone still
   * narrowed its grid on attach and on every ResizeObserver wake, while each
   * arriving snapshot restored the owner's width. That alternation is what the
   * operator reported as "terminal is unstable, keeps jumping back time. I am
   * required to do redraws". Redraw was a workaround for a fight between two
   * code paths.
   *
   * Measuring is always safe and is what the server needs to hear; whether the
   * claim is granted is the server's to decide, and the grid that results
   * arrives as a snapshot.
   */
  async #measureForResize(): Promise<{ rows: number; columns: number } | undefined> {
    if (this.#started && this.#connection.ownsGeometry === false) {
      return this.#surface.proposeFit?.();
    }
    return this.#surface.fit();
  }

  #mayResizeNow(): boolean {
    return !this.#disposed && this.#attached && this.#connection.ownsGeometry !== false;
  }

  async #refitAttachedSurface(intent: "operator" | "echo" = "operator"): Promise<void> {
    // A phone hides and shows its address bar as the operator scrolls, which
    // resizes the container and wakes the observer. The old comment claiming
    // the container "never changed" was true of a column-count change and false
    // of a phone, so this path re-shredded the terminal on every scroll.
    const measured = await this.#measureForResize();
    if (!measured) return;
    if (this.#disposed || !this.#started || !this.#host.parentElement) return;
    this.#connection.resize(measured.rows, measured.columns, intent);
  }

  async #fitAndStart(): Promise<void> {
    // The surface owns the ResizeObserver, so it has to know this too. A
    // predicate rather than a value: ownership changes mid-session when another
    // device takes or releases the claim, and a copied boolean goes stale.
    this.#surface.observeGeometryOwnership?.(() => this.#connection.ownsGeometry !== false);
    // No canonical screen exists yet. Initial fit waits for usable metrics;
    // once attached, passive views only measure and accept engine dimensions.
    const measured = await this.#measureForResize();
    if (!measured) return;
    const { rows, columns } = measured;
    if (this.#disposed || this.#started || !this.#host.parentElement) return;
    // Connecting is not a claim; the resume frame carries that intent.
    this.#connection.resize(rows, columns, "echo");
    this.#connection.start({
      onOutput: (bytes) => this.#disposed ? Promise.resolve() : this.#surface.write(bytes),
      onSnapshot: async (snapshot) => {
        if (this.#disposed) return;
        const restoreFocus = document.activeElement === this.#host
          || Boolean(document.activeElement && this.#host.contains(document.activeElement));
        await this.#surface.restore(snapshot);
        if (this.#disposed || !this.#attached) return;
        // Passive views always accept canonical geometry. Neither a snapshot
        // nor a viewport resize is an implicit request to take control.
        if (documentHasFocus() && this.#connection.ownsGeometry !== false) {
          try {
            const fitted = await this.#measureForResize();
            if (fitted && this.#mayResizeNow()) {
              this.#connection.resize(fitted.rows, fitted.columns, "echo");
            }
          } catch {
            // Keep the canonical screen when a transient layout cannot fit.
          }
        }
        this.#applyRestoredFocus(restoreFocus);
      },
      onState: (state, detail) => this.#setState(state, detail),
      onControlChange: (control) => {
        for (const subscriber of this.#controlSubscribers) subscriber(control);
      },
      onRunningChange: (running) => {
        if (!running) this.#setState("closed", "worker process exited");
      },
    });
    this.#surfaceSubscriptions.push(
      this.#surface.onResize(({ rows: nextRows, columns: nextColumns, origin }) =>
        this.#connection.resize(nextRows, nextColumns, origin === "restore" ? "echo" : "operator"),
      ),
    );
    this.#started = true;
    this.#applyPendingFocus();
  }

  #applyRestoredFocus(restoreFocus: boolean): void {
    if (restoreFocus && this.#lastRequestedFocus) this.#pendingFocus = this.#lastRequestedFocus;
    this.#applyPendingFocus();
  }

  #setState(state: TerminalConnectionState, detail?: string): void {
    if (this.#disposed) return;
    this.#state = state;
    this.#lifecycle?.stateChanged(state);
    this.#stateDetail = detail;
    if (state === "connected" && this.#focusOnConnect) {
      this.#pendingFocus = this.#focusOnConnect;
      this.#focusOnConnect = undefined;
      this.#applyPendingFocus();
    }
    for (const subscriber of this.#statusSubscribers) subscriber(state, detail);
  }

  #setAtBottom(atBottom: boolean): void {
    if (this.#atBottom === atBottom) return;
    this.#atBottom = atBottom;
    for (const subscriber of this.#scrollSubscribers) subscriber(atBottom);
  }

  #applyPendingFocus(): void {
    if (!this.#pendingFocus || !this.#host.parentElement || !this.#opened || !this.#started) return;
    const focus = this.#pendingFocus;
    if (focus === "input") this.#surface.focus();
    else this.#host.focus({ preventScroll: true });
    const activeElement = document.activeElement;
    if (activeElement === this.#host || (activeElement && this.#host.contains(activeElement))) {
      this.#pendingFocus = undefined;
    }
  }
}

export class TerminalControllerRegistry {
  readonly #controllers = new Map<string, TerminalController>();
  #retainedLimit: number | undefined;
  #evictions = 0;
  readonly #recentlyEvicted = new Set<string>();
  readonly #restoreEvidence = new TerminalRestoreEvidence();

  /** Experimental browser-only retention. Undefined preserves the current behavior. */
  setRetainedLimit(limit: number | undefined): void {
    if (limit !== undefined && (!Number.isInteger(limit) || limit < 1 || limit > 64)) {
      throw new Error("Terminal renderer limit must be an integer between 1 and 64");
    }
    if (limit !== this.#retainedLimit) {
      if (limit === undefined) this.#restoreEvidence.stop();
      else {
        this.#restoreEvidence.reset();
        this.#evictions = 0;
      }
      this.#recentlyEvicted.clear();
    }
    this.#retainedLimit = limit;
    this.#trim();
  }

  get retention() {
    const attached = [...this.#controllers.values()].filter((controller) => controller.attached).length;
    return { limit: this.#retainedLimit, retained: this.size, attached, inactive: this.size - attached, evictions: this.#evictions };
  }

  get coldRestoreEvidence() { return this.#restoreEvidence.snapshot(); }

  getOrCreate(
    sessionId: string,
    surfaceFactory: TerminalSurfaceFactory,
    connectionFactory: TerminalConnectionFactory,
  ): TerminalController {
    const existing = this.#controllers.get(sessionId);
    if (existing) {
      this.#touch(sessionId);
      return existing;
    }
    const cold = this.#recentlyEvicted.delete(sessionId);
    let completed = false;
    let finish: ReturnType<TerminalRestoreEvidence["begin"]> | undefined;
    const controller = new TerminalController(surfaceFactory, connectionFactory, {
      attached: () => {
        if (cold && !completed && !finish && this.#retainedLimit !== undefined && document.visibilityState === "visible") {
          finish = this.#restoreEvidence.begin();
        }
        this.#touch(sessionId);
        this.#trim(sessionId);
      },
      inactive: () => { finish?.("interrupted"); finish = undefined; },
      stateChanged: (state) => {
        if (state === "connected") {
          finish?.(document.visibilityState === "visible" ? "rendered" : "interrupted");
          completed = true;
          finish = undefined;
        } else if (state === "error" || state === "closed" || state === "recovery_required") {
          finish?.("failed");
          completed = true;
          finish = undefined;
        }
      },
    });
    this.#controllers.set(sessionId, controller);
    // Keep the requested controller alive until its view can attach. During a
    // handoff the previous view may still be mounted; attachment trims again.
    this.#trim(sessionId);
    return controller;
  }

  get(sessionId: string): TerminalController | undefined {
    return this.#controllers.get(sessionId);
  }

  closeSession(sessionId: string): void {
    const controller = this.#controllers.get(sessionId);
    if (!controller) return;
    controller.dispose();
    this.#controllers.delete(sessionId);
  }

  closeAll(): void {
    for (const controller of this.#controllers.values()) controller.dispose();
    this.#controllers.clear();
    this.#evictions = 0;
    this.#recentlyEvicted.clear();
    this.#restoreEvidence.reset();
  }

  get size(): number {
    return this.#controllers.size;
  }

  #touch(sessionId: string): void {
    const controller = this.#controllers.get(sessionId);
    if (!controller) return;
    this.#controllers.delete(sessionId);
    this.#controllers.set(sessionId, controller);
  }

  #trim(protectedSession?: string): void {
    if (this.#retainedLimit === undefined) return;
    for (const [id, controller] of this.#controllers) {
      if (this.size <= this.#retainedLimit) break;
      if (id === protectedSession || controller.attached) continue;
      this.closeSession(id);
      this.#evictions += 1;
      this.#recentlyEvicted.add(id);
      if (this.#recentlyEvicted.size > 64) this.#recentlyEvicted.delete(this.#recentlyEvicted.values().next().value!);
    }
  }
}

/**
 * Whether this window is the one the operator is acting in.
 *
 * Exactly one window has focus browser-wide, which is what makes it usable to
 * pick a single geometry owner among viewers the server sees as one device.
 * `visibilityState` cannot: a pop-out and its opener are both visible.
 */
function documentHasFocus(): boolean {
  return typeof document === "undefined" || typeof document.hasFocus !== "function" || document.hasFocus();
}
