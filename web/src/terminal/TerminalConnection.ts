import { BROWSER_SESSION_AUTH } from "../api";
import { presenceDeviceId } from "../presence/PresenceController";

export type TerminalConnectionState =
  | "connecting"
  | "connected"
  | "disconnected"
  | "closed"
  | "recovery_required"
  | "error";

export interface TerminalConnectionHandlers {
  onOutput(bytes: Uint8Array): void | Promise<void>;
  onSnapshot(snapshot: TerminalSnapshot): void | Promise<void>;
  onState(state: TerminalConnectionState, detail?: string): void;
  onRunningChange(running: boolean): void;
}

export interface TerminalSnapshot {
  sequence: number;
  rows: number;
  columns: number;
  truncated: boolean;
  bytes: Uint8Array;
}

type GrantResponse = {
  grant: string;
  protocol: string;
  websocket_path: string;
  expires_in_ms: number;
};

type WebSocketFactory = (url: string, protocols: string[]) => WebSocket;

export interface TerminalConnectionOptions {
  sessionId: string;
  operatorToken: string;
  fetch?: typeof window.fetch;
  websocketFactory?: WebSocketFactory;
  locationOrigin?: string;
  retryDelaysMs?: readonly number[];
  confirmationTimeoutMs?: number;
  deviceId?: string;
}

const GRANT_PROTOCOL_PREFIX = "swarm-grant.";
const OUTPUT_FRAME_TYPE = 1;
const SNAPSHOT_FRAME_TYPE = 2;
const MAX_PENDING_RENDER_BYTES = 3 * 1024 * 1024;
// Match the bounded application bootstrap recovery window so an API-only
// rolling update cannot leave an already-open terminal permanently detached.
const DEFAULT_RETRY_DELAYS_MS = [100, 250, 500, 1_000, 2_000, 4_000, 8_000] as const;
const DEFAULT_CONFIRMATION_TIMEOUT_MS = 3_000;
// Browsers may send only 1000 or application-owned 3000-4999 close codes.
const CLOSE_PROTOCOL_FAILURE = 4008;
const CLOSE_FRESH_SNAPSHOT = 4013;

/** Owns one browser attachment independently from React component lifetime. */
export class TerminalConnection {
  readonly #sessionId: string;
  readonly #operatorToken: string;
  readonly #fetch: typeof window.fetch;
  readonly #websocketFactory: WebSocketFactory;
  readonly #locationOrigin: string;
  readonly #retryDelaysMs: readonly number[];
  readonly #confirmationTimeoutMs: number;
  readonly #deviceId: string;
  #handlers: TerminalConnectionHandlers | undefined;
  #socket: WebSocket | undefined;
  #retryTimer: ReturnType<typeof setTimeout> | undefined;
  #confirmationTimer: ReturnType<typeof setTimeout> | undefined;
  #grantAbortController: AbortController | undefined;
  #retryAttempt = 0;
  #sequence = 0;
  #hasCanonicalState = false;
  #renderQueue = Promise.resolve();
  #pendingRenderBytes = 0;
  #renderGeneration = 0;
  #started = false;
  #disposed = false;
  #fatal = false;
  #recovering = false;
  #connectionConfirmed = false;
  #rendererConfirmed = false;
  #processExited = false;
  #size: { rows: number; columns: number } | undefined;

  constructor(options: TerminalConnectionOptions) {
    this.#sessionId = options.sessionId;
    this.#operatorToken = options.operatorToken;
    this.#fetch = options.fetch ?? window.fetch.bind(window);
    this.#websocketFactory = options.websocketFactory ?? ((url, protocols) => new WebSocket(url, protocols));
    this.#locationOrigin = options.locationOrigin ?? window.location.origin;
    this.#retryDelaysMs = options.retryDelaysMs ?? DEFAULT_RETRY_DELAYS_MS;
    this.#confirmationTimeoutMs = options.confirmationTimeoutMs ?? DEFAULT_CONFIRMATION_TIMEOUT_MS;
    this.#deviceId = options.deviceId ?? presenceDeviceId();
  }

  start(handlers: TerminalConnectionHandlers): void {
    if (this.#disposed) throw new Error("Cannot start a disposed terminal connection");
    if (this.#started) return;
    if (!this.#size) throw new Error("Cannot start a terminal connection before measuring its renderer");
    this.#started = true;
    this.#handlers = handlers;
    void this.#connect();
  }

  sendInput(text: string): void {
    this.#send({ type: "input", text });
  }

  resize(rows: number, columns: number): void {
    if (rows <= 0 || columns <= 0) return;
    this.#size = { rows, columns };
    this.#send({ type: "resize", rows, columns });
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    if (this.#retryTimer !== undefined) clearTimeout(this.#retryTimer);
    this.#retryTimer = undefined;
    this.#clearConfirmationTimer();
    this.#grantAbortController?.abort();
    this.#grantAbortController = undefined;
    this.#socket?.close(1000, "terminal controller disposed");
    this.#socket = undefined;
    this.#handlers?.onState("closed");
  }

  get sequence(): number {
    return this.#sequence;
  }

  async #connect(): Promise<void> {
    if (this.#disposed || this.#fatal) return;
    this.#handlers?.onState("connecting");
    const grantAbortController = new AbortController();
    this.#grantAbortController = grantAbortController;
    this.#armGrantTimer(grantAbortController);
    try {
      const response = await this.#fetch(
        `/api/v1/terminal/sessions/${encodeURIComponent(this.#sessionId)}/attach-grants`,
        {
          method: "POST",
          headers: this.#operatorToken === BROWSER_SESSION_AUTH
            ? undefined
            : { Authorization: `Bearer ${this.#operatorToken}` },
          credentials: "same-origin",
          cache: "no-store",
          signal: grantAbortController.signal,
        },
      );
      if (!response.ok) throw new Error(`Attach grant returned ${response.status}`);
      const grant = (await response.json()) as GrantResponse;
      if (this.#disposed || this.#grantAbortController !== grantAbortController) return;
      this.#grantAbortController = undefined;
      this.#clearConfirmationTimer();
      const websocketUrl = new URL(grant.websocket_path, this.#locationOrigin);
      websocketUrl.protocol = websocketUrl.protocol === "https:" ? "wss:" : "ws:";
      const socket = this.#websocketFactory(websocketUrl.toString(), [
        grant.protocol,
        `${GRANT_PROTOCOL_PREFIX}${grant.grant}`,
      ]);
      socket.binaryType = "arraybuffer";
      this.#connectionConfirmed = false;
      this.#rendererConfirmed = false;
      this.#recovering = false;
      this.#socket = socket;
      this.#armConfirmationTimer(socket);
      socket.addEventListener("open", () => this.#handleOpen(socket));
      socket.addEventListener("message", (event) => this.#handleMessage(socket, event));
      socket.addEventListener("close", () => this.#handleClose(socket));
      socket.addEventListener("error", () => this.#handleSocketError(socket));
    } catch (error) {
      if (this.#grantAbortController !== grantAbortController) return;
      this.#grantAbortController = undefined;
      this.#clearConfirmationTimer();
      this.#scheduleReconnect(error instanceof Error ? error.message : "terminal attachment failed");
    }
  }

  #handleOpen(socket: WebSocket): void {
    if (socket !== this.#socket || this.#disposed) return;
    const size = this.#size;
    if (!size) {
      this.#fail("terminal renderer size was unavailable during attachment");
      return;
    }
    socket.send(
      JSON.stringify({
        type: "resume",
        after_sequence: this.#hasCanonicalState ? this.#sequence : null,
        rows: size.rows,
        columns: size.columns,
        device_id: this.#deviceId,
        // The selected foreground terminal is the operator's current viewport.
        // Claiming only during a visible attachment lets refresh and worker
        // selection repair stale geometry without allowing a background PWA
        // to fight the active desktop or mobile view.
        claim_geometry: document.visibilityState === "visible",
      }),
    );
  }

  #handleMessage(socket: WebSocket, event: MessageEvent): void {
    if (socket !== this.#socket || this.#disposed) return;
    if (event.data instanceof ArrayBuffer) {
      this.#enqueueBinaryFrame(new Uint8Array(event.data));
      return;
    }
    if (typeof event.data !== "string") {
      this.#fail("unsupported terminal WebSocket message");
      return;
    }
    let message: { type?: string; running?: boolean; latest_sequence?: number; code?: string; message?: string };
    try {
      message = JSON.parse(event.data) as typeof message;
    } catch {
      this.#fail("terminal WebSocket returned invalid JSON");
      return;
    }
    if (message.type === "state" && typeof message.running === "boolean") {
      if (this.#hasCanonicalState) this.#confirmRenderedConnection();
      this.#processExited = !message.running;
      this.#handlers?.onRunningChange(message.running);
    } else if (message.type === "error") {
      this.#handlers?.onState("error", message.message ?? message.code ?? "terminal protocol error");
    }
  }

  #enqueueBinaryFrame(frame: Uint8Array): void {
    if (this.#recovering) return;
    if (this.#pendingRenderBytes + frame.byteLength > MAX_PENDING_RENDER_BYTES) {
      this.#recoverFromSnapshot("terminal renderer fell behind its bounded queue");
      return;
    }
    this.#pendingRenderBytes += frame.byteLength;
    const generation = this.#renderGeneration;
    this.#renderQueue = this.#renderQueue
      .then(async () => {
        if (generation !== this.#renderGeneration || this.#disposed) return;
        await this.#applyBinaryFrame(frame);
      })
      .catch((error: unknown) => {
        this.#fail(error instanceof Error ? error.message : "terminal renderer failed");
      })
      .finally(() => {
        this.#pendingRenderBytes -= frame.byteLength;
      });
  }

  async #applyBinaryFrame(frame: Uint8Array): Promise<void> {
    if (frame.byteLength < 9) {
      this.#fail("terminal output frame was malformed");
      return;
    }
    const view = new DataView(frame.buffer, frame.byteOffset + 1, 8);
    const sequence = Number(view.getBigUint64(0));
    if (!Number.isSafeInteger(sequence)) {
      this.#fail("terminal sequence exceeded browser precision");
      return;
    }
    if (frame[0] === SNAPSHOT_FRAME_TYPE) {
      if (frame.byteLength < 14) {
        this.#fail("terminal snapshot frame was malformed");
        return;
      }
      const dimensions = new DataView(frame.buffer, frame.byteOffset + 9, 4);
      const rows = dimensions.getUint16(0);
      const columns = dimensions.getUint16(2);
      if (rows === 0 || columns === 0) {
        this.#fail("terminal snapshot dimensions were invalid");
        return;
      }
      if (this.#hasCanonicalState && sequence < this.#sequence) return;
      const truncated = frame[13] === 1;
      await this.#handlers?.onSnapshot({
        sequence,
        rows,
        columns,
        truncated,
        bytes: frame.slice(14),
      });
      this.#sequence = sequence;
      this.#hasCanonicalState = true;
      if (truncated) {
        this.#confirmRenderedConnection(
          "Terminal view was reset after exceeding its canonical memory bound",
        );
      } else {
        this.#confirmRenderedConnection();
      }
      return;
    }
    if (frame[0] !== OUTPUT_FRAME_TYPE) {
      this.#fail("terminal binary frame type was unsupported");
      return;
    }
    if (!this.#hasCanonicalState) {
      this.#fail("terminal output arrived before canonical state");
      return;
    }
    if (sequence <= this.#sequence) return;
    if (sequence !== this.#sequence + 1) {
      this.#recoverFromSnapshot(
        `terminal sequence gap: expected ${this.#sequence + 1}, received ${sequence}`,
      );
      return;
    }
    await this.#handlers?.onOutput(frame.slice(9));
    this.#sequence = sequence;
    this.#confirmRenderedConnection();
  }

  #handleClose(socket: WebSocket): void {
    if (socket !== this.#socket) return;
    this.#clearConfirmationTimer();
    this.#socket = undefined;
    if (!this.#disposed && !this.#fatal && !this.#processExited) this.#scheduleReconnect("terminal connection closed");
  }

  #handleSocketError(socket: WebSocket): void {
    if (socket === this.#socket && !this.#disposed) {
      this.#handlers?.onState("disconnected", "terminal connection error");
    }
  }

  #scheduleReconnect(detail: string): void {
    if (this.#disposed || this.#fatal || this.#retryTimer !== undefined) return;
    const delay = this.#retryDelaysMs[this.#retryAttempt];
    if (delay === undefined) {
      this.#handlers?.onState("error", `${detail}; reconnect limit reached`);
      return;
    }
    this.#retryAttempt += 1;
    this.#handlers?.onState("disconnected", detail);
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = undefined;
      void this.#connect();
    }, delay);
  }

  #send(message: object): void {
    if (this.#socket?.readyState === WebSocket.OPEN) this.#socket.send(JSON.stringify(message));
  }

  #fail(detail: string): void {
    this.#fatal = true;
    this.#renderGeneration += 1;
    this.#clearConfirmationTimer();
    this.#handlers?.onState("recovery_required", detail);
    this.#socket?.close(CLOSE_PROTOCOL_FAILURE, detail.slice(0, 100));
  }

  #recoverFromSnapshot(detail: string): void {
    if (this.#recovering || this.#fatal || this.#disposed) return;
    this.#recovering = true;
    this.#renderGeneration += 1;
    this.#hasCanonicalState = false;
    this.#sequence = 0;
    this.#handlers?.onState("disconnected", `${detail}; requesting a fresh snapshot`);
    this.#socket?.close(CLOSE_FRESH_SNAPSHOT, "fresh terminal snapshot required");
  }

  #confirmConnection(): void {
    if (this.#connectionConfirmed) return;
    this.#connectionConfirmed = true;
    this.#retryAttempt = 0;
  }

  #confirmRenderedConnection(detail?: string): void {
    this.#clearConfirmationTimer();
    this.#confirmConnection();
    if (this.#rendererConfirmed && detail === undefined) return;
    this.#rendererConfirmed = true;
    this.#handlers?.onState("connected", detail);
  }

  #armConfirmationTimer(socket: WebSocket): void {
    this.#clearConfirmationTimer();
    this.#confirmationTimer = setTimeout(() => {
      this.#confirmationTimer = undefined;
      if (socket !== this.#socket || this.#disposed || this.#fatal) return;
      this.#socket = undefined;
      socket.close(CLOSE_FRESH_SNAPSHOT, "terminal confirmation timed out");
      this.#scheduleReconnect("terminal connection received no confirmation");
    }, this.#confirmationTimeoutMs);
  }

  #armGrantTimer(controller: AbortController): void {
    this.#clearConfirmationTimer();
    this.#confirmationTimer = setTimeout(() => {
      this.#confirmationTimer = undefined;
      if (controller !== this.#grantAbortController || this.#disposed || this.#fatal) return;
      this.#grantAbortController = undefined;
      controller.abort();
      this.#scheduleReconnect("terminal attach grant received no response");
    }, this.#confirmationTimeoutMs);
  }

  #clearConfirmationTimer(): void {
    if (this.#confirmationTimer !== undefined) clearTimeout(this.#confirmationTimer);
    this.#confirmationTimer = undefined;
  }
}
