import {
  TerminalController,
  TerminalControllerRegistry,
  type TerminalConnectionFactory,
  type TerminalSurfaceFactory,
} from "./TerminalController";

/** Application-lifetime owner kept outside React component lifecycle. */
export class TerminalWorkspace {
  readonly #controllers = new TerminalControllerRegistry();
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
    return this.#controllers.getOrCreate(sessionId, surfaceFactory, connectionFactory);
  }

  closeSession(sessionId: string): void {
    this.#controllers.closeSession(sessionId);
  }

  logout(): void {
    this.#controllers.closeAll();
    this.#operatorToken = undefined;
  }
}

export const terminalWorkspace = new TerminalWorkspace();
