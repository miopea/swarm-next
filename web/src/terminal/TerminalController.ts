export interface TerminalSurface {
  open(element: HTMLElement): void;
  dispose(): void;
}

export type TerminalSurfaceFactory = () => TerminalSurface;

/** Owns a renderer independently from any React component. */
export class TerminalController {
  readonly #host = document.createElement("div");
  readonly #surface: TerminalSurface;
  #opened = false;
  #disposed = false;

  constructor(factory: TerminalSurfaceFactory) {
    this.#surface = factory();
    this.#host.className = "terminal-surface";
  }

  attach(container: HTMLElement): void {
    if (this.#disposed) throw new Error("Cannot attach a disposed terminal");
    if (!this.#opened) {
      this.#surface.open(this.#host);
      this.#opened = true;
    }
    container.replaceChildren(this.#host);
  }

  detach(): void {
    this.#host.remove();
  }

  dispose(): void {
    if (this.#disposed) return;
    this.detach();
    this.#surface.dispose();
    this.#disposed = true;
  }
}

export class TerminalControllerRegistry {
  readonly #controllers = new Map<string, TerminalController>();

  getOrCreate(sessionId: string, factory: TerminalSurfaceFactory): TerminalController {
    const existing = this.#controllers.get(sessionId);
    if (existing) return existing;
    const controller = new TerminalController(factory);
    this.#controllers.set(sessionId, controller);
    return controller;
  }

  closeSession(sessionId: string): void {
    const controller = this.#controllers.get(sessionId);
    if (!controller) return;
    controller.dispose();
    this.#controllers.delete(sessionId);
  }

  get size(): number {
    return this.#controllers.size;
  }
}
