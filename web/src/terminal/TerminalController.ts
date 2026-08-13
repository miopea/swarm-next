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
  onResize(listener: (size: { rows: number; columns: number }) => void): Disposable;
  dispose(): void;
}

export interface TerminalConnectionLike {
  start(handlers: TerminalConnectionHandlers): void;
  sendInput(text: string): void;
  resize(rows: number, columns: number): void;
  dispose(): void;
}

export type TerminalSurfaceFactory = () => TerminalSurface;
export type TerminalConnectionFactory = () => TerminalConnectionLike;
export type TerminalStatusListener = (state: TerminalConnectionState, detail?: string) => void;

const MAX_STATUS_SUBSCRIBERS = 8;

/** Owns renderer and transport lifecycles independently from React views. */
export class TerminalController {
  readonly #host = document.createElement("div");
  readonly #surface: TerminalSurface;
  readonly #connection: TerminalConnectionLike;
  readonly #surfaceSubscriptions: Disposable[];
  readonly #statusSubscribers = new Set<TerminalStatusListener>();
  #opened = false;
  #started = false;
  #startPromise: Promise<void> | undefined;
  #refitPromise: Promise<void> | undefined;
  #disposed = false;
  #state: TerminalConnectionState = "connecting";
  #stateDetail: string | undefined;
  #pendingFocus: "container" | "input" | undefined;
  #focusOnConnect: "container" | "input" | undefined;

  constructor(surfaceFactory: TerminalSurfaceFactory, connectionFactory: TerminalConnectionFactory) {
    this.#surface = surfaceFactory();
    this.#connection = connectionFactory();
    this.#host.className = "terminal-surface";
    this.#host.tabIndex = -1;
    this.#surfaceSubscriptions = [
      this.#surface.onData((text) => this.#connection.sendInput(text)),
    ];
  }

  attach(container: HTMLElement): void {
    if (this.#disposed) throw new Error("Cannot attach a disposed terminal");
    container.replaceChildren(this.#host);
    if (!this.#opened) {
      this.#surface.open(this.#host);
      this.#opened = true;
    }
    if (this.#started) {
      this.#applyPendingFocus();
      this.#refitWhenAttached();
    } else {
      this.#startWhenFitted();
    }
  }

  detach(): void {
    this.#host.remove();
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
    this.#focusOnConnect = this.#state === "connected" ? undefined : this.#pendingFocus;
    this.#applyPendingFocus();
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.detach();
    for (const subscription of this.#surfaceSubscriptions) subscription.dispose();
    this.#connection.dispose();
    this.#surface.dispose();
    this.#statusSubscribers.clear();
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
    const refitPromise = this.#refitAttachedSurface();
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

  async #refitAttachedSurface(): Promise<void> {
    const { rows, columns } = await this.#surface.fit();
    if (this.#disposed || !this.#started || !this.#host.parentElement) return;
    this.#connection.resize(rows, columns);
  }

  async #fitAndStart(): Promise<void> {
    const { rows, columns } = await this.#surface.fit();
    if (this.#disposed || this.#started || !this.#host.parentElement) return;
    this.#connection.resize(rows, columns);
    this.#connection.start({
      onOutput: (bytes) => this.#surface.write(bytes),
      onSnapshot: async (snapshot) => {
        await this.#surface.restore(snapshot);
        try {
          const fitted = await this.#surface.fit();
          this.#connection.resize(fitted.rows, fitted.columns);
        } catch {
          // Responsive PWA transitions can briefly leave the mounted surface
          // without measurable font metrics. The canonical snapshot is already
          // restored; ResizeObserver will publish the settled dimensions.
        }
        this.#applyPendingFocus();
      },
      onState: (state, detail) => this.#setState(state, detail),
      onRunningChange: (running) => {
        if (!running) this.#setState("closed", "worker process exited");
      },
    });
    this.#surfaceSubscriptions.push(
      this.#surface.onResize(({ rows: nextRows, columns: nextColumns }) =>
        this.#connection.resize(nextRows, nextColumns),
      ),
    );
    this.#started = true;
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
