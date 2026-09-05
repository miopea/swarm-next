import {
  TerminalController,
  TerminalControllerRegistry,
  type TerminalConnectionFactory,
  type TerminalSurfaceFactory,
} from "./TerminalController";
import { terminalDraft } from "./TerminalDraft";

/** Application-lifetime owner kept outside React component lifecycle. */
export class TerminalWorkspace {
  readonly #controllers = new TerminalControllerRegistry();
  readonly #pendingFocus = new Map<string, boolean>();
  #operatorToken: string | undefined;

  authenticate(operatorToken: string): void {
    if (operatorToken === this.#operatorToken) return;
    if (this.#operatorToken !== undefined) terminalDraft.clear();
    this.#controllers.closeAll();
    this.#operatorToken = operatorToken;
  }

  /** Opt-in experiment; applies only to this browser application's renderers. */
  setWarmPoolExperiment(enabled: boolean): void {
    this.#controllers.setRetainedLimit(enabled ? 5 : undefined);
  }

  get rendererRetention() { return this.#controllers.retention; }
  get coldRestoreEvidence() { return this.#controllers.coldRestoreEvidence; }

  /** Call only with a successful authoritative session read, not a failed fetch. */
  reconcileSessions(runningSessionIds: Iterable<string>): void {
    const running = new Set(runningSessionIds);
    this.#controllers.reconcileSessions(running);
    for (const id of this.#pendingFocus.keys()) {
      if (!running.has(id)) this.#pendingFocus.delete(id);
    }
  }

  controllerFor(
    sessionId: string,
    surfaceFactory: TerminalSurfaceFactory,
    connectionFactory: TerminalConnectionFactory,
  ): TerminalController {
    if (!this.#operatorToken) throw new Error("Terminal workspace is not authenticated");
    const controller = this.#controllers.getOrCreate(sessionId, surfaceFactory, connectionFactory);
    const focusInput = this.#pendingFocus.get(sessionId);
    if (focusInput !== undefined) {
      this.#pendingFocus.delete(sessionId);
      controller.requestFocus(focusInput);
    }
    return controller;
  }

  /**
   * Refit a live terminal to this device's viewport.
   *
   * Used after taking a worker: the claim moves the geometry to this device,
   * but nothing on screen changes until something re-fits, so "Work here" left
   * the terminal at the size the device you took it from had set.
   */
  redrawSession(sessionId: string): void {
    void this.#controllers.get(sessionId)?.redraw();
  }

  focusSession(sessionId: string, input: boolean): void {
    const controller = this.#controllers.get(sessionId);
    if (controller) controller.requestFocus(input);
    else this.#pendingFocus.set(sessionId, input);
  }

  /**
   * Discard only the browser renderer and its socket. The durable worker and
   * provider process remain owned by the terminal host; the next view mount
   * reconnects and restores the host's canonical snapshot.
   */
  resetSessionRenderer(sessionId: string): void {
    this.#controllers.closeSession(sessionId);
  }

  closeSession(sessionId: string): void {
    this.#pendingFocus.delete(sessionId);
    this.#controllers.closeSession(sessionId);
  }

  logout(): void {
    terminalDraft.clear();
    this.#controllers.closeAll();
    this.#controllers.setRetainedLimit(undefined);
    this.#pendingFocus.clear();
    this.#operatorToken = undefined;
  }
}

export const terminalWorkspace = new TerminalWorkspace();
