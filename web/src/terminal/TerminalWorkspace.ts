import {
  TerminalController,
  TerminalControllerRegistry,
  type TerminalConnectionFactory,
  type TerminalSurfaceFactory,
} from "./TerminalController";

/** Application-lifetime owner kept outside React component lifecycle. */
export class TerminalWorkspace {
  readonly #controllers = new TerminalControllerRegistry();
  readonly #pendingFocus = new Map<string, boolean>();
  #operatorToken: string | undefined;

  authenticate(operatorToken: string): void {
    if (operatorToken === this.#operatorToken) return;
    this.#controllers.closeAll();
    this.#operatorToken = operatorToken;
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

  focusSession(sessionId: string, input: boolean): void {
    const controller = this.#controllers.get(sessionId);
    if (controller) controller.requestFocus(input);
    else this.#pendingFocus.set(sessionId, input);
  }

  closeSession(sessionId: string): void {
    this.#pendingFocus.delete(sessionId);
    this.#controllers.closeSession(sessionId);
  }

  logout(): void {
    this.#controllers.closeAll();
    this.#pendingFocus.clear();
    this.#operatorToken = undefined;
  }
}

export const terminalWorkspace = new TerminalWorkspace();
