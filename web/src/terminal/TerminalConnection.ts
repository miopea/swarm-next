import { BROWSER_SESSION_AUTH } from "../api";
import { presenceDeviceId } from "../presence/PresenceController";
import { browserPerformance } from "../runtime/browserPerformance";
import { TerminalControl } from "./TerminalControl";

export type TerminalControlView = "checking" | "owned" | "available" | "elsewhere" | "unsupported";

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
  onControlChange?(control: TerminalControlView): void;
}

/**
 * Why the canonical screen is being rebuilt.
 *
 * The operator sees a cover over the terminal while this happens, and it used
 * to say "adjusting layout" whatever the cause — so a recovery from a burst of
 * build output read as an unexplained interruption. The reason travels with
 * the snapshot so the cover can say which of these actually happened.
 */
export type SnapshotReason = "attached" | "fell_behind" | "dropped_output" | "reattached";

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
const MAX_PENDING_RENDER_FRAMES = 1_024;
const MAX_OUTPUT_BATCH_BYTES = 64 * 1024;
type QueuedRender = { frame: Uint8Array; generation: number; enqueuedAt: number };

function outputSequence(frame: Uint8Array): number | undefined {
  if (frame.byteLength < 9 || frame[0] !== OUTPUT_FRAME_TYPE) return undefined;
  const sequence = Number(new DataView(frame.buffer, frame.byteOffset + 1, 8).getBigUint64(0));
  return Number.isSafeInteger(sequence) ? sequence : undefined;
}
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
  /**
   * Whether a surface is currently on screen to render into.
   *
   * xterm's write callback confirms parsing, not browser paint. Retained views
   * must still avoid parsing a backlog nobody is viewing. On return, a fresh
   * canonical snapshot supplies the newest screen instead of playback. Pending
   * callbacks from the prior visible period cannot confirm the restored view.
   */
  #rendering = true;
  /** Whether anything was dropped while detached, so the screen is now stale. */
  #missedWhileDetached = false;
  #hasCanonicalState = false;
  /** Input and geometry share the engine's generation-bound owner. */
  readonly #control = new TerminalControl();
  readonly #viewId = crypto.randomUUID();
  #controlView: TerminalControlView = "checking";
  #controlReceived = false;
  #renewTimer: ReturnType<typeof setTimeout> | undefined;
  #renderFrames: QueuedRender[] = [];
  #renderDraining = false;
  #pendingRenderFrames = 0;
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
  #attachStartedAt: number | undefined;
  #processExited = false;
  #size: { rows: number; columns: number } | undefined;
  #probeId: string | undefined;

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
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", this.#handleVisibilityChange);
      window.addEventListener("focus", this.#handleVisibilityChange);
      window.addEventListener("blur", this.#handleVisibilityChange);
    }
    void this.#connect();
  }

  /** True means accepted by the browser socket, not acknowledged by the provider. */
  sendInput(text: string): boolean {
    const generation = this.#control.inputGeneration;
    if (this.#disposed || this.#fatal || !this.#foreground() || !this.#rendererConfirmed || generation === undefined) return false;
    return this.#send({ type: "input", generation, text });
  }

  /** Whether this device may set the terminal's size. */
  get ownsGeometry(): boolean {
    return this.#control.ownsControl && this.#foreground();
  }

  get controlView(): TerminalControlView { return this.#controlView; }

  resumeHere(rows: number, columns: number): boolean {
    const generation = this.#control.observedGeneration;
    if (!this.#foreground() || generation === undefined || rows <= 0 || columns <= 0) return false;
    this.#size = { rows, columns };
    const sent = this.#send({ type: "claim", observed_generation: generation, rows, columns });
    if (sent) this.#unconfirmControl();
    return sent;
  }

  /** Explicit worker navigation releases control; merely hiding a PWA does not. */
  releaseControl(): void {
    const generation = this.#control.inputGeneration;
    if (generation !== undefined) this.#send({ type: "release", generation });
    this.#unconfirmControl();
  }

  #foreground(): boolean {
    return this.#rendering && document.visibilityState === "visible" && document.hasFocus();
  }

  #publishControl(): void {
    const next: TerminalControlView = !this.#control.confirmed ? "checking"
      : !this.#control.status.supported ? "unsupported"
      : this.#control.ownsControl ? "owned"
      : this.#control.status.occupied ? "elsewhere" : "available";
    if (next !== this.#controlView) {
      this.#controlView = next;
      this.#handlers?.onControlChange?.(next);
    }
    if (this.#renewTimer !== undefined && (!this.#foreground() || !this.#control.ownsControl)) {
      clearTimeout(this.#renewTimer);
      this.#renewTimer = undefined;
    }
    if (this.#renewTimer === undefined && this.#foreground() && this.#control.ownsControl) {
      this.#renewTimer = setTimeout(() => {
        this.#renewTimer = undefined;
        const generation = this.#control.inputGeneration;
        if (generation !== undefined && this.#foreground()) {
          if (!this.#send({ type: "renew", generation })) this.#unconfirmControl();
        }
      }, 30_000);
    }
  }

  #unconfirmControl(): void {
    this.#control.disconnect();
    this.#publishControl();
  }

  /** Measuring never claims ownership; only Resume Here can displace a view. */
  resize(rows: number, columns: number, _intent: "operator" | "echo" = "operator"): void {
    if (rows <= 0 || columns <= 0) return;
    this.#size = { rows, columns };
    const generation = this.#control.inputGeneration;
    if (!this.#foreground() || generation === undefined) return;
    this.#send({
      type: "resize",
      rows,
      columns,
      generation,
    });
  }

  dispose(): void {
    if (this.#disposed) return;
    this.releaseControl();
    this.#disposed = true;
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", this.#handleVisibilityChange);
      window.removeEventListener("focus", this.#handleVisibilityChange);
      window.removeEventListener("blur", this.#handleVisibilityChange);
    }
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
    this.#unconfirmControl();
    this.#probeId = undefined;
    this.#attachStartedAt ??= performance.now();
    this.#handlers?.onState("connecting");
    const grantAbortController = new AbortController();
    this.#grantAbortController = grantAbortController;
    this.#armGrantTimer(grantAbortController);
    try {
      const response = await this.#fetch(
        `/api/v1/terminal/sessions/${encodeURIComponent(this.#sessionId)}/attach-grants?protocol=swarm-terminal.v4`,
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
      if (grant.protocol !== "swarm-terminal.v4") {
        this.#fail("Update the Swarm App/API to enable safe terminal control. This client will not use legacy input.");
        return;
      }
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

  /// Re-checks the socket when the tab comes back, because a frozen page
  /// cannot have noticed it dying.
  ///
  /// A backgrounded mobile tab is FROZEN: timers stop and no JavaScript runs.
  /// If the connection is dropped during that freeze, the close event is never
  /// delivered — there is nothing running to deliver it. On resume
  /// `readyState` still reads OPEN, so the client believes it is connected to
  /// a socket the network discarded long ago. Nothing arrives, nothing retries,
  /// and the only move left is the one the operator described: "I have to
  /// refresh to get it to load."
  ///
  /// OPEN means "nobody has told us otherwise", which is not the same as alive.
  /// A correlated probe uses the same bounded confirmation timer as attachment.
  /// A live socket answers without resizing or copying a terminal snapshot;
  /// a dead one does not, and after the timeout that timer closes it and
  /// reconnects through the ordinary path. No new recovery machinery, and a
  /// healthy connection pays one message.
  #handleVisibilityChange = (): void => {
    if (this.#disposed || this.#fatal) return;
    this.#unconfirmControl();
    if (!this.#foreground()) return;
    const socket = this.#socket;
    // Anything not OPEN is already the reconnect path's business; asking again
    // here would double it.
    if (!socket || socket.readyState !== WebSocket.OPEN || this.#probeId !== undefined) return;
    this.#probeId = crypto.randomUUID();
    this.#armConfirmationTimer(socket);
    if (!this.#send({ type: "probe", request_id: this.#probeId })) this.#handleSocketError(socket);
  };

  #handleOpen(socket: WebSocket): void {
    if (socket !== this.#socket || this.#disposed) return;
    this.#sendResume(socket);
  }

  /// The resume frame, sent from the one place that knows its shape.
  ///
  /// Sent exactly once per socket. A second resume is a protocol violation;
  /// visibility checks use probes instead.
  #sendResume(socket: WebSocket): void {
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
        // Distinct views on the same device cannot share input authority.
        // Foreground attachment may acquire an empty owner, never displace one.
        view_id: this.#viewId,
        foreground: this.#foreground(),
      }),
    );
  }

  #handleMessage(socket: WebSocket, event: MessageEvent): void {
    if (socket !== this.#socket || this.#disposed || this.#fatal) return;
    if (event.data instanceof ArrayBuffer) {
      this.#enqueueBinaryFrame(new Uint8Array(event.data));
      return;
    }
    if (typeof event.data !== "string") {
      this.#fail("unsupported terminal WebSocket message");
      return;
    }
    let message: { type?: string; request_id?: string; running?: boolean; latest_sequence?: number; control?: unknown; code?: string; message?: string };
    try {
      message = JSON.parse(event.data) as typeof message;
    } catch {
      this.#fail("terminal WebSocket returned invalid JSON");
      return;
    }
    if (message.type === "alive") {
      if (this.#probeId !== undefined && message.request_id === this.#probeId) {
        this.#probeId = undefined;
        if (this.#hasCanonicalState) this.#confirmRenderedConnection();
        // A transport reply is not ownership evidence. Ask the engine without
        // displacing another view; its reply alone can restore permission.
        if (!this.#controlReceived || this.#control.status.supported) {
          if (this.#foreground() && this.#size) this.#send({ type: "claim", observed_generation: null, ...this.#size });
        } else if (this.#controlView === "checking") {
          this.#control.observe(this.#control.status);
          this.#publishControl();
        }
      }
    } else if (message.type === "control" || (message.type === "state" && typeof message.running === "boolean")) {
      if (this.#control.observe(message.control) === "invalid") {
        this.#fail("The engine returned invalid terminal ownership. Input is disabled.");
        return;
      }
      this.#controlReceived = true;
      this.#publishControl();
      if (message.type === "control") return;
      if (this.#hasCanonicalState) this.#confirmRenderedConnection();
      this.#processExited = !message.running;
      this.#handlers?.onRunningChange(message.running!);
    } else if (message.type === "error") {
      this.#unconfirmControl();
      // The following engine status explains an ownership refusal. It is not
      // a transport failure and no input is retried.
      if (["terminal_control_owned_elsewhere", "terminal_control_stale", "terminal_control_expired"].includes(message.code ?? "")) return;
      if (message.code === "terminal_engine_update_required") {
        this.#control.observe({ supported: false, generation: null, owned: false, occupied: false, lease_remaining_ms: 0 });
        this.#publishControl();
        return;
      }
      if (this.#probeId !== undefined && message.code === "invalid_message") {
        // Terminal adapter owns this early-v4 compatibility path. Remove
        // when probe support is the minimum API: safe reattachment only.
        // Never repeat resume or input on the existing socket.
        this.#probeId = undefined;
        this.#clearConfirmationTimer();
        this.#socket = undefined;
        socket.close(CLOSE_FRESH_SNAPSHOT, "terminal probe requires reattachment");
        this.#scheduleReconnect("refreshing terminal connection");
        return;
      }
      this.#handlers?.onState("error", message.message ?? message.code ?? "terminal protocol error");
    }
  }

  /**
   * Stops queueing frames that nothing can draw.
   *
   * Dropping rather than buffering is safe because the recovery path asks for a
   * canonical snapshot, and a snapshot carries the scrollback as well as the
   * visible screen — so nothing is lost that replaying would have preserved.
   * It is also the operator's stated expectation: the terminal should be live
   * unless they scrolled up themselves.
   */
  suspendRendering(): void {
    if (this.#rendering && this.#pendingRenderBytes > 0) {
      // Frames accepted while visible may still be waiting for the parser.
      // Retire those callbacks as well as dropping newly arriving frames. The
      // next visible view restores canonical state instead of playing them back.
      this.#renderGeneration += 1;
      this.#missedWhileDetached = true;
    }
    this.#rendering = false;
    this.#unconfirmControl();
  }

  /** Back on screen: take a snapshot if anything happened while it was away. */
  resumeRendering(): void {
    if (this.#rendering) return;
    this.#rendering = true;
    this.#handleVisibilityChange();
    if (!this.#missedWhileDetached) return;
    this.#missedWhileDetached = false;
    this.#recoverFromSnapshot("reattached", "terminal is catching up to live");
  }

  #enqueueBinaryFrame(frame: Uint8Array): void {
    if (this.#recovering) return;
    if (!this.#rendering) {
      // Nothing can draw this, and holding it only decides how long the
      // catch-up takes when the operator returns.
      this.#missedWhileDetached = true;
      return;
    }
    if (this.#pendingRenderBytes + frame.byteLength > MAX_PENDING_RENDER_BYTES
      || this.#pendingRenderFrames >= MAX_PENDING_RENDER_FRAMES) {
      this.#recoverFromSnapshot("fell_behind", "terminal renderer fell behind its bounded queue");
      return;
    }
    this.#pendingRenderBytes += frame.byteLength;
    this.#pendingRenderFrames += 1;
    this.#renderFrames.push({ frame, generation: this.#renderGeneration, enqueuedAt: performance.now() });
    if (this.#renderDraining) return;
    this.#renderDraining = true;
    queueMicrotask(() => { void this.#drainRenderFrames(); });
  }

  async #drainRenderFrames(): Promise<void> {
    try {
      while (this.#renderFrames.length) {
        const first = this.#renderFrames.shift()!;
        const { generation, enqueuedAt } = first;
        let frame = first.frame;
        let consumedBytes = frame.byteLength;
        let consumedFrames = 1;
        let lastSequence: number | undefined;
        try {
          if (generation !== this.#renderGeneration || this.#disposed) continue;
          const firstSequence = outputSequence(frame);
          if (this.#hasCanonicalState && firstSequence === this.#sequence + 1) {
            // Only consecutive deltas can share one parser write. Snapshots,
            // duplicates, gaps and malformed frames keep their own validation.
            // Do not wait for more output: batch only what is already queued.
            lastSequence = firstSequence;
            const parts = [frame.subarray(9)];
            let payloadBytes = frame.byteLength - 9;
            while (this.#renderFrames.length) {
              const next = this.#renderFrames[0];
              if (next.generation !== generation || outputSequence(next.frame) !== lastSequence + 1
                || payloadBytes + next.frame.byteLength - 9 > MAX_OUTPUT_BATCH_BYTES) break;
              this.#renderFrames.shift();
              parts.push(next.frame.subarray(9));
              payloadBytes += next.frame.byteLength - 9;
              consumedBytes += next.frame.byteLength;
              consumedFrames += 1;
              lastSequence += 1;
            }
            if (parts.length > 1) {
              const combined = new Uint8Array(9 + payloadBytes);
              combined.set(frame.subarray(0, 9));
              let offset = 9;
              for (const part of parts) { combined.set(part, offset); offset += part.byteLength; }
              frame = combined;
            }
          }
          await this.#applyBinaryFrame(frame, generation, lastSequence);
          if (!this.#disposed && generation === this.#renderGeneration && document.visibilityState === "visible") {
            browserPerformance.record("terminal_render", performance.now() - enqueuedAt);
          }
        } catch (error) {
          if (generation === this.#renderGeneration && !this.#disposed) {
            this.#fail(error instanceof Error ? error.message : "terminal renderer failed");
          }
        } finally {
          this.#pendingRenderBytes -= consumedBytes;
          this.#pendingRenderFrames -= consumedFrames;
        }
      }
    } finally { this.#renderDraining = false; }
  }

  async #applyBinaryFrame(frame: Uint8Array, generation: number, lastSequence?: number): Promise<void> {
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
      if (generation !== this.#renderGeneration || this.#disposed) return;
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
    if (generation !== this.#renderGeneration || this.#disposed) return;
    this.#sequence = lastSequence ?? sequence;
    this.#confirmRenderedConnection();
  }

  #handleClose(socket: WebSocket): void {
    if (socket !== this.#socket) return;
    this.#clearConfirmationTimer();
    this.#socket = undefined;
    this.#unconfirmControl();
    if (!this.#disposed && !this.#fatal && !this.#processExited) this.#scheduleReconnect("terminal connection closed");
  }

  #handleSocketError(socket: WebSocket): void {
    if (socket === this.#socket && !this.#disposed) {
      this.#unconfirmControl();
      this.#handlers?.onState("disconnected", "terminal connection error");
    }
  }

  /**
   * Backs off, and keeps backing off — it does not stop.
   *
   * Running off the end of the ladder used to report an error and schedule
   * nothing, and since #retryAttempt is only reset by a CONFIRMED connection,
   * nothing could ever restart it. The terminal was dead until the page was
   * reloaded. The operator hit this repeatedly: "this randomly happens and the
   * only way to fix it is to force close or reload", with the banner reading
   * "terminal attach grant received no response; reconnect limit reached". The
   * whole ladder spans under sixteen seconds, so any absence longer than that —
   * an API restart, a machine briefly loaded — spent the budget and gave up.
   *
   * What the bound is actually for is the open-close loop: a socket that opens
   * and closes at once must not hammer the host. Holding at the ladder's last
   * delay keeps that protection, because the rate stays bounded; it is the
   * giving up that was never the point. The state stays "error" so a long
   * failure is still visible rather than looking like a routine blip.
   */
  #scheduleReconnect(detail: string): void {
    this.#unconfirmControl();
    if (this.#disposed || this.#fatal || this.#retryTimer !== undefined) return;
    const ladder = this.#retryDelaysMs;
    const exhausted = this.#retryAttempt >= ladder.length;
    const delay = exhausted ? ladder[ladder.length - 1] : ladder[this.#retryAttempt];
    if (delay === undefined) {
      // Only reachable with an empty ladder, which is a caller asking for no
      // retries at all.
      this.#handlers?.onState("error", `${detail}; not retrying`);
      return;
    }
    this.#retryAttempt += 1;
    this.#handlers?.onState(
      exhausted ? "error" : "disconnected",
      exhausted ? `${detail}; still retrying every ${Math.round(delay / 1000)}s` : detail,
    );
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = undefined;
      void this.#connect();
    }, delay);
  }

  #send(message: object): boolean {
    if (this.#socket?.readyState !== WebSocket.OPEN) return false;
    try {
      this.#socket.send(JSON.stringify(message));
      return true;
    } catch {
      return false;
    }
  }

  #fail(detail: string): void {
    this.#unconfirmControl();
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
    if (this.#probeId === undefined) this.#clearConfirmationTimer();
    this.#confirmConnection();
    if (this.#rendererConfirmed && detail === undefined) return;
    if (this.#attachStartedAt !== undefined) {
      if (document.visibilityState === "visible") browserPerformance.record("terminal_reconnect", performance.now() - this.#attachStartedAt);
      this.#attachStartedAt = undefined;
    }
    this.#rendererConfirmed = true;
    this.#handlers?.onState("connected", detail);
  }

  #armConfirmationTimer(socket: WebSocket): void {
    this.#clearConfirmationTimer();
    this.#confirmationTimer = setTimeout(() => {
      this.#confirmationTimer = undefined;
      if (socket !== this.#socket || this.#disposed || this.#fatal) return;
      this.#probeId = undefined;
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
