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

/**
 * Why the canonical screen is being rebuilt.
 *
 * The operator sees a cover over the terminal while this happens, and it used
 * to say "adjusting layout" whatever the cause — so a recovery from a burst of
 * build output read as an unexplained interruption. The reason travels with
 * the snapshot so the cover can say which of these actually happened.
 */
export type SnapshotReason = "attached" | "fell_behind" | "dropped_output";

export interface TerminalSnapshot {
  sequence: number;
  rows: number;
  columns: number;
  truncated: boolean;
  reason: SnapshotReason;
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
/**
 * What a refused attach grant means, in words rather than a status code.
 *
 * A 502, 503 or 504 here is the ordinary shape of an update: the API is
 * restarting and cannot hand out a socket for a few seconds. Reporting
 * "Attach grant returned 503" made a routine reload read as a fault — the
 * operator's words, watching a worker come back: "It works, but a bit of a
 * false flag."
 *
 * The reconnect behaviour is unchanged; only what the operator is told about
 * it. A status this does not recognise keeps its number, because an unexpected
 * one is worth being able to look up.
 */
export function attachGrantFailure(status: number): string {
  if (status === 502 || status === 503 || status === 504) {
    return "Swarm is restarting; reconnecting";
  }
  if (status === 401 || status === 403) {
    return "this terminal is no longer authorized";
  }
  if (status === 404) {
    return "this terminal session is no longer on the worker engine";
  }
  return `Swarm could not attach this terminal (${status})`;
}

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
  /**
   * Whether this device may set the terminal's size, as the server last said.
   *
   * Assumed true until told otherwise so a fresh connection still sizes itself.
   * A device that has lost the claim must stop re-asserting its own size: it
   * cannot win, and each attempt costs a canonical snapshot that resizes the
   * screen back.
   */
  #geometryOwned = true;
  #renderQueue = Promise.resolve();
  #pendingRenderBytes = 0;
  #renderGeneration = 0;
  #started = false;
  #disposed = false;
  #fatal = false;
  #recovering = false;
  // Survives the socket rebuild that recovery triggers, so the snapshot that
  // arrives afterwards can say what it is recovering from.
  #recoveryReason: SnapshotReason | undefined;
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

  /** Whether this device may set the terminal's size. */
  get ownsGeometry(): boolean {
    return this.#geometryOwned;
  }

  /**
   * @param intent `"operator"` when a person changed this viewport — resizing
   * the window, selecting the worker, pressing refresh. `"echo"` when the
   * renderer is re-fitting to a size that arrived from somewhere else.
   *
   * Only an operator's own change asks to take authority over the PTY. An echo
   * asking to claim is what made two devices trade a terminal's size: measured
   * on 2026-08-23, 31 of 31 requests in one 68-second window asked to take
   * authority, four of them succeeded in taking it from the other device, and
   * nine were refused — including the operator's own attempts to take over,
   * which is why taking over needed several tries.
   */
  resize(rows: number, columns: number, intent: "operator" | "echo" = "operator"): void {
    if (rows <= 0 || columns <= 0) return;
    this.#size = { rows, columns };
    this.#send({
      type: "resize",
      rows,
      columns,
      // Focus rather than visibility, because a pop-out and the window it came
      // from are both visible at once and would each claim the same PTY.
      claim_geometry: intent === "operator" && document.hasFocus(),
    });
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
      if (!response.ok) throw new Error(attachGrantFailure(response.status));
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
        // The selected terminal in the focused window is the operator's
        // current viewport. Claiming on attachment lets refresh and worker
        // selection repair stale geometry, and keying on focus rather than
        // visibility stops a pop-out and its opener — which the server sees as
        // one device — from each claiming the same PTY on connect.
        claim_geometry: document.hasFocus(),
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
    let message: { type?: string; running?: boolean; latest_sequence?: number; geometry_owned?: boolean; code?: string; message?: string };
    try {
      message = JSON.parse(event.data) as typeof message;
    } catch {
      this.#fail("terminal WebSocket returned invalid JSON");
      return;
    }
    if (message.type === "state" && typeof message.running === "boolean") {
      if (typeof message.geometry_owned === "boolean") this.#geometryOwned = message.geometry_owned;
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
      this.#recoverFromSnapshot("fell_behind", "terminal renderer fell behind its bounded queue");
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
      const reason = this.#recoveryReason ?? "attached";
      this.#recoveryReason = undefined;
      await this.#handlers?.onSnapshot({
        sequence,
        rows,
        columns,
        truncated,
        reason,
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
        "dropped_output",
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

  #recoverFromSnapshot(reason: SnapshotReason, detail: string): void {
    if (this.#recovering || this.#fatal || this.#disposed) return;
    this.#recovering = true;
    this.#recoveryReason = reason;
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
