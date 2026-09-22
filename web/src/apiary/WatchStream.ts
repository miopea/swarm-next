/**
 * One read-only window into another Hive's terminal.
 *
 * ⚠️ DELIBERATELY NOT `TerminalConnection`. That class owns attach grants,
 * input, engagement leases and control handover, none of which apply here — a
 * watch is looking, not touching, and reusing it would have brought a write
 * path into a surface that must not have one. The frame FORMAT is shared, which
 * is the part worth sharing.
 *
 * Nothing here retains frames. What arrives is handed straight to the renderer.
 */

const OUTPUT_FRAME_TYPE = 1;
const SNAPSHOT_FRAME_TYPE = 2;
const GRANT_PROTOCOL_PREFIX = "swarm-watch.";
/**
 * How long to wait for the watched Hive to accept.
 *
 * Its federation pass is paced at sixty seconds, so anything shorter routinely
 * declares a healthy watch dead.
 */
const ACKNOWLEDGEMENT_WAIT_MS = 60_000;
const ACKNOWLEDGEMENT_POLL_MS = 2_000;

export type WatchStreamState = "connecting" | "live" | "closed";

export interface WatchSnapshot {
  rows: number;
  columns: number;
  truncated: boolean;
  bytes: Uint8Array;
}

export interface WatchStreamHandlers {
  onOutput(bytes: Uint8Array): void;
  onSnapshot(snapshot: WatchSnapshot): void;
  onState(state: WatchStreamState, detail?: string): void;
}

export type WebSocketFactory = (url: string, protocols: string[]) => WebSocket;

export interface WatchStreamOptions {
  watchId: string;
  operatorToken: string;
  handlers: WatchStreamHandlers;
  fetch?: typeof window.fetch;
  websocketFactory?: WebSocketFactory;
  locationOrigin?: string;
}

export class WatchStream {
  readonly #options: WatchStreamOptions;
  #socket: WebSocket | undefined;
  #closed = false;

  constructor(options: WatchStreamOptions) {
    this.#options = options;
  }

  /**
   * Fetches a single-use grant, then opens the socket with it.
   *
   * ⚠️ TWO STEPS BECAUSE A BROWSER CANNOT SEND AN `Authorization` HEADER ON A
   * WEBSOCKET. The grant is carried as a subprotocol instead — short-lived and
   * single-use, so a value that lands in a proxy log is worth nothing a moment
   * later. The operator token never goes near the socket.
   */
  async open(): Promise<void> {
    const { watchId, operatorToken, handlers } = this.#options;
    handlers.onState("connecting");
    const origin = this.#options.locationOrigin ?? window.location.origin;
    const request = this.#options.fetch ?? window.fetch.bind(window);
    // ⚠️ A WATCH IS NOT LIVE UNTIL THE WATCHED HIVE ACKNOWLEDGES IT, and that
    // Hive learns on its own federation pass. Asking once and giving up
    // reported "no longer live" for a watch a second old — a working feature
    // reading as a broken one, which is exactly what the field saw on
    // 2026-09-22. The takeover window already waited; this one did not.
    let grant: string | undefined;
    let path: string | undefined;
    const deadline = Date.now() + ACKNOWLEDGEMENT_WAIT_MS;
    while (!this.#closed && grant === undefined) {
      let status = 0;
      try {
        const response = await request(`${origin}/api/v1/apiary/watches/${encodeURIComponent(watchId)}/grant`, {
          method: "POST",
          headers: { Authorization: `Bearer ${operatorToken}` },
        });
        status = response.status;
        if (response.ok) {
          const body = (await response.json()) as { grant?: string; websocket_path?: string };
          if (body?.grant && body?.websocket_path) {
            grant = body.grant;
            path = body.websocket_path;
            break;
          }
        }
      } catch {
        handlers.onState("closed", "This Hive could not be reached.");
        return;
      }
      if (Date.now() >= deadline) {
        handlers.onState("closed", status === 403
          ? "That Hive did not accept the watch."
          : watchGrantFailure(status));
        return;
      }
      handlers.onState("connecting", "Waiting for that Hive to accept…");
      await new Promise((resolve) => { setTimeout(resolve, ACKNOWLEDGEMENT_POLL_MS); });
    }
    if (this.#closed || grant === undefined || path === undefined) return;

    const url = `${origin.replace(/^http/, "ws")}${path}`;
    const factory = this.#options.websocketFactory ?? ((target, protocols) => new WebSocket(target, protocols));
    const socket = factory(url, [`${GRANT_PROTOCOL_PREFIX}${grant}`]);
    socket.binaryType = "arraybuffer";
    this.#socket = socket;
    socket.addEventListener("open", () => handlers.onState("live"));
    socket.addEventListener("message", (event) => this.#receive(event as MessageEvent));
    // ⚠️ A CLOSED WINDOW IS NOT AN ERROR. The watch may simply have ended, or
    // the watched operator may have stopped it from their own machine — which
    // they are entitled to do at any moment. Saying "the window closed" is
    // honest; saying "connection failed" would blame the network for a
    // decision somebody made.
    socket.addEventListener("close", () => handlers.onState("closed", "The window closed."));
    socket.addEventListener("error", () => handlers.onState("closed", "The window closed."));
  }

  close(): void {
    this.#closed = true;
    this.#socket?.close();
    this.#socket = undefined;
  }

  #receive(event: MessageEvent): void {
    if (!(event.data instanceof ArrayBuffer)) return;
    const frame = new Uint8Array(event.data);
    // A frame too short to carry its own header is dropped rather than parsed
    // out of whatever bytes happen to follow.
    if (frame.byteLength < 9) return;
    if (frame[0] === OUTPUT_FRAME_TYPE) {
      this.#options.handlers.onOutput(frame.slice(9));
      return;
    }
    if (frame[0] === SNAPSHOT_FRAME_TYPE && frame.byteLength >= 14) {
      const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
      this.#options.handlers.onSnapshot({
        rows: view.getUint16(9),
        columns: view.getUint16(11),
        truncated: frame[13] === 1,
        bytes: frame.slice(14),
      });
    }
  }
}

/** What a refused grant means, in words rather than a status code. */
export function watchGrantFailure(status: number): string {
  if (status === 403) return "That watch is no longer live.";
  if (status === 429) return "Too many windows are open right now.";
  if (status >= 500) return "This Hive is restarting; try again in a moment.";
  return `The window could not be opened (${status}).`;
}
