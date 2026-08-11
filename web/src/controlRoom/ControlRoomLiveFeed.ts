import { fetchControlRoomEvents, type ControlRoomEventPage } from "../api";

export type LiveFeedState = "connecting" | "connected" | "retrying";
type FetchPage = (operatorToken: string, after: number, signal: AbortSignal) => Promise<ControlRoomEventPage>;
type Delay = (milliseconds: number, signal: AbortSignal) => Promise<void>;

const RETRY_DELAYS_MS = [250, 500, 1_000, 2_000, 5_000] as const;

export class ControlRoomLiveFeed {
  #controller?: AbortController;

  constructor(
    private readonly fetchPage: FetchPage = fetchControlRoomEvents,
    private readonly delay: Delay = abortableDelay,
  ) {}

  start(
    operatorToken: string,
    onInvalidate: () => Promise<void>,
    onStateChange: (state: LiveFeedState) => void = () => undefined,
  ) {
    this.stop();
    const controller = new AbortController();
    this.#controller = controller;
    void this.#run(operatorToken, controller.signal, onInvalidate, onStateChange);
  }

  stop() {
    this.#controller?.abort();
    this.#controller = undefined;
  }

  async #run(
    operatorToken: string,
    signal: AbortSignal,
    onInvalidate: () => Promise<void>,
    onStateChange: (state: LiveFeedState) => void,
  ) {
    let cursor = 0;
    let failures = 0;
    onStateChange("connecting");
    while (!signal.aborted) {
      try {
        const page = await this.fetchPage(operatorToken, cursor, signal);
        if (signal.aborted) return;
        if (page.reset_required || page.events.length > 0) await onInvalidate();
        cursor = page.next_cursor;
        failures = 0;
        onStateChange("connected");
      } catch (error) {
        if (signal.aborted || (error instanceof DOMException && error.name === "AbortError")) return;
        onStateChange("retrying");
        const delay = RETRY_DELAYS_MS[Math.min(failures, RETRY_DELAYS_MS.length - 1)];
        failures += 1;
        await this.delay(delay, signal).catch(() => undefined);
      }
    }
  }
}

function abortableDelay(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(new DOMException("Aborted", "AbortError"));
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, milliseconds);
    const abort = () => {
      window.clearTimeout(timer);
      reject(new DOMException("Aborted", "AbortError"));
    };
    signal.addEventListener("abort", abort, { once: true });
  });
}