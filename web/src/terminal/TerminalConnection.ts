export type TerminalConnectionState =
  | "connecting"
  | "connected"
  | "disconnected"
  | "closed"
  | "recovery_required"
  | "error";

export interface TerminalConnectionHandlers {
  onOutput(bytes: Uint8Array): void;
  onState(state: TerminalConnectionState, detail?: string): void;
  onRunningChange(running: boolean): void;
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
}

const GRANT_PROTOCOL_PREFIX = "swarm-grant.";
const OUTPUT_FRAME_TYPE = 1;
const DEFAULT_RETRY_DELAYS_MS = [100, 250, 500, 1_000, 2_000] as const;

/** Owns one browser attachment independently from React component lifetime. */
export class TerminalConnection {
  readonly #sessionId: string;
  readonly #operatorToken: string;
  readonly #fetch: typeof window.fetch;
  readonly #websocketFactory: WebSocketFactory;
  readonly #locationOrigin: string;
  readonly #retryDelaysMs: readonly number[];
  #handlers: TerminalConnectionHandlers | undefined;
  #socket: WebSocket | undefined;
  #retryTimer: ReturnType<typeof setTimeout> | undefined;
  #retryAttempt = 0;
  #sequence = 0;
  #started = false;
  #disposed = false;
  #fatal = false;
  #connectionConfirmed = false;

  constructor(options: TerminalConnectionOptions) {
    this.#sessionId = options.sessionId;
    this.#operatorToken = options.operatorToken;
    this.#fetch = options.fetch ?? window.fetch.bind(window);
    this.#websocketFactory = options.websocketFactory ?? ((url, protocols) => new WebSocket(url, protocols));
    this.#locationOrigin = options.locationOrigin ?? window.location.origin;
    this.#retryDelaysMs = options.retryDelaysMs ?? DEFAULT_RETRY_DELAYS_MS;
  }

  start(handlers: TerminalConnectionHandlers): void {
    if (this.#disposed) throw new Error("Cannot start a disposed terminal connection");
    if (this.#started) return;
    this.#started = true;
    this.#handlers = handlers;
    void this.#connect();
  }

  sendInput(text: string): void {
    this.#send({ type: "input", text });
  }

  resize(rows: number, columns: number): void {
    if (rows <= 0 || columns <= 0) return;
    this.#send({ type: "resize", rows, columns });
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    if (this.#retryTimer !== undefined) clearTimeout(this.#retryTimer);
    this.#retryTimer = undefined;
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
    try {
      const response = await this.#fetch(
        `/api/v1/terminal/sessions/${encodeURIComponent(this.#sessionId)}/attach-grants`,
        {
          method: "POST",
          headers: { Authorization: `Bearer ${this.#operatorToken}` },
          cache: "no-store",
        },
      );
      if (!response.ok) throw new Error(`Attach grant returned ${response.status}`);
      const grant = (await response.json()) as GrantResponse;
      if (this.#disposed) return;
      const websocketUrl = new URL(grant.websocket_path, this.#locationOrigin);
      websocketUrl.protocol = websocketUrl.protocol === "https:" ? "wss:" : "ws:";
      const socket = this.#websocketFactory(websocketUrl.toString(), [
        grant.protocol,
        `${GRANT_PROTOCOL_PREFIX}${grant.grant}`,
      ]);
      socket.binaryType = "arraybuffer";
      this.#connectionConfirmed = false;
      this.#socket = socket;
      socket.addEventListener("open", () => this.#handleOpen(socket));
      socket.addEventListener("message", (event) => this.#handleMessage(socket, event));
      socket.addEventListener("close", () => this.#handleClose(socket));
      socket.addEventListener("error", () => this.#handleSocketError(socket));
    } catch (error) {
      this.#scheduleReconnect(error instanceof Error ? error.message : "terminal attachment failed");
    }
  }

  #handleOpen(socket: WebSocket): void {
    if (socket !== this.#socket || this.#disposed) return;
    socket.send(JSON.stringify({ type: "resume", after_sequence: this.#sequence }));
    this.#handlers?.onState("connected");
  }

  #handleMessage(socket: WebSocket, event: MessageEvent): void {
    if (socket !== this.#socket || this.#disposed) return;
    if (event.data instanceof ArrayBuffer) {
      this.#handleOutputFrame(new Uint8Array(event.data));
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
      this.#confirmConnection();
      this.#handlers?.onRunningChange(message.running);
    } else if (message.type === "snapshot_required") {
      this.#fatal = true;
      this.#handlers?.onState(
        "recovery_required",
        `Terminal history no longer contains sequence ${this.#sequence + 1}`,
      );
      socket.close(1008, "canonical snapshot required");
    } else if (message.type === "error") {
      this.#handlers?.onState("error", message.message ?? message.code ?? "terminal protocol error");
    }
  }

  #handleOutputFrame(frame: Uint8Array): void {
    if (frame.byteLength < 9 || frame[0] !== OUTPUT_FRAME_TYPE) {
      this.#fail("terminal output frame was malformed");
      return;
    }
    const view = new DataView(frame.buffer, frame.byteOffset + 1, 8);
    const sequence = Number(view.getBigUint64(0));
    if (!Number.isSafeInteger(sequence)) {
      this.#fail("terminal sequence exceeded browser precision");
      return;
    }
    if (sequence <= this.#sequence) return;
    if (sequence !== this.#sequence + 1) {
      this.#fail(`terminal sequence gap: expected ${this.#sequence + 1}, received ${sequence}`);
      return;
    }
    this.#sequence = sequence;
    this.#confirmConnection();
    this.#handlers?.onOutput(frame.slice(9));
  }

  #handleClose(socket: WebSocket): void {
    if (socket !== this.#socket) return;
    this.#socket = undefined;
    if (!this.#disposed && !this.#fatal) this.#scheduleReconnect("terminal connection closed");
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
    this.#handlers?.onState("recovery_required", detail);
    this.#socket?.close(1008, detail.slice(0, 100));
  }

  #confirmConnection(): void {
    if (this.#connectionConfirmed) return;
    this.#connectionConfirmed = true;
    this.#retryAttempt = 0;
  }
}
