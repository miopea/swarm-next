import type {
  TerminalConnectionHandlers,
  TerminalConnectionState,
  TerminalSnapshot,
} from "./TerminalConnection";

export interface Disposable {
  dispose(): void;
}

export interface TerminalSurface {
  open(element: HTMLElement): void;
  focus(): void;
  fit(): Promise<{ rows: number; columns: number }>;
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
  sendInput(text: string): void;
  resize(rows: number, columns: number, intent?: "operator" | "echo"): void;
  dispose(): void;
  /**
   * Whether this device may set the terminal's size. Optional so a test double
   * that does not model the claim behaves as it did before — owning it.
   */
  readonly ownsGeometry?: boolean;
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

  constructor(surfaceFactory: TerminalSurfaceFactory, connectionFactory: TerminalConnectionFactory) {
    this.#surface = surfaceFactory();
    this.#connection = connectionFactory();
    this.#host.className = "terminal-surface";
    this.#host.tabIndex = -1;
    this.#surfaceSubscriptions = [
      this.#surface.onData((text) => this.#connection.sendInput(text)),
      this.#surface.onScroll((atBottom) => this.#setAtBottom(atBottom)),
      this.#surface.onRenderable?.((renderable) => {
        this.#visible = renderable;
        this.#updateRendering();
      }) ?? { dispose: () => undefined },
    ];
  }

  attach(container: HTMLElement): void {
    if (this.#disposed) throw new Error("Cannot attach a disposed terminal");
    container.replaceChildren(this.#host);
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
    this.#updateRendering();
    this.#host.remove();
  }

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

  sendInput(text: string): void {
    if (this.#disposed) throw new Error("Cannot send input to a disposed terminal");
    this.#connection.sendInput(text);
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

  async #refitAttachedSurface(intent: "operator" | "echo" = "operator"): Promise<void> {
    const { rows, columns } = await this.#surface.fit();
    if (this.#disposed || !this.#started || !this.#host.parentElement) return;
    this.#connection.resize(rows, columns, intent);
  }

  async #fitAndStart(): Promise<void> {
    const { rows, columns } = await this.#surface.fit();
    if (this.#disposed || this.#started || !this.#host.parentElement) return;
    // Connecting is not a claim; the resume frame carries that intent.
    this.#connection.resize(rows, columns, "echo");
    this.#connection.start({
      onOutput: (bytes) => this.#surface.write(bytes),
      onSnapshot: async (snapshot) => {
        const restoreFocus = document.activeElement === this.#host
          || Boolean(document.activeElement && this.#host.contains(document.activeElement));
        await this.#surface.restore(snapshot);
        // Only the window the operator is actually in re-asserts its own size.
        //
        // Two viewers of one PTY on the same machine are one device — the
        // device id lives in localStorage and is shared by every window and tab
        // — so the server cannot tell them apart and applies both their
        // resizes. Each then restores at the other's size, re-fits to its own,
        // and resizes back: a pop-out and its opener adjust the terminal
        // forever. `hasFocus` picks exactly one window browser-wide, so the
        // other accepts the canonical size instead of arguing with it.
        //
        // Ungated below this: the fit before `start`, so a fresh mount still
        // sizes itself, and ResizeObserver, which reports a real viewport
        // change rather than an echo of someone else's.
        try {
          // Two gates, for two different fights.
          //
          // Focus settles the one between a pop-out and the window it came
          // from: they share a device id, so the server sees one device and
          // applies both their resizes.
          //
          // Ownership settles the one between separate machines. A phone opened
          // on a worker left running on a desktop has focus and has lost the
          // claim, so focus alone let it re-fit, be refused, take the canonical
          // size, and re-fit again — the terminal jumped continuously and the
          // operator could not use it. A device that does not own the geometry
          // accepts the size it is given; typing takes the claim, which is what
          // moving to another device is supposed to mean.
          if (!documentHasFocus() || this.#connection.ownsGeometry === false) {
            return this.#applyRestoredFocus(restoreFocus);
          }
          const fitted = await this.#surface.fit();
          this.#connection.resize(fitted.rows, fitted.columns, "echo");
        } catch {
          // Responsive PWA transitions can briefly leave the mounted surface
          // without measurable font metrics. The canonical snapshot is already
          // restored; ResizeObserver will publish the settled dimensions.
        }
        this.#applyRestoredFocus(restoreFocus);
      },
      onState: (state, detail) => this.#setState(state, detail),
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
    this.#state = state;
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

  getOrCreate(
    sessionId: string,
    surfaceFactory: TerminalSurfaceFactory,
    connectionFactory: TerminalConnectionFactory,
  ): TerminalController {
    const existing = this.#controllers.get(sessionId);
    if (existing) return existing;
    const controller = new TerminalController(surfaceFactory, connectionFactory);
    this.#controllers.set(sessionId, controller);
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
  }

  get size(): number {
    return this.#controllers.size;
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
