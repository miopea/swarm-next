import { fetchControlRoomEvents, type ControlRoomEventPage } from "../api";

export type LiveFeedState = "connecting" | "connected" | "retrying";
type FetchPage = (operatorToken: string, after: number, signal: AbortSignal) => Promise<ControlRoomEventPage>;
type Delay = (milliseconds: number, signal: AbortSignal) => Promise<void>;

const RETRY_DELAYS_MS = [250, 500, 1_000, 2_000, 5_000] as const;
const ORDINARY_INVALIDATION_SETTLE_MS = 250;

/// How long one poll may take before it is treated as hung rather than slow.
///
/// The server holds an unanswered poll for at most twenty seconds, so anything
/// past this is not a slow answer. A backgrounded mobile tab can leave an
/// in-flight fetch that never settles at all — it neither resolves nor rejects
/// — and without a ceiling the loop waits on it forever while the last state it
/// published stays "connected". The roster then shows work frozen where it
/// stood, and nothing the operator does moves it.
const POLL_CEILING_MS = 35_000;

export class ControlRoomLiveFeed {
  #controller?: AbortController;

  constructor(
    private readonly fetchPage: FetchPage = fetchControlRoomEvents,
    private readonly delay: Delay = abortableDelay,
    private readonly pollCeilingMs: number = POLL_CEILING_MS,
  ) {}

  start(
    operatorToken: string,
    onInvalidate: (page: ControlRoomEventPage, signal: AbortSignal) => Promise<void>,
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
    onInvalidate: (page: ControlRoomEventPage, signal: AbortSignal) => Promise<void>,
    onStateChange: (state: LiveFeedState) => void,
  ) {
    let cursor = 0;
    let failures = 0;
    onStateChange("connecting");
    while (!signal.aborted) {
      const poll = new AbortController();
      const abortPoll = () => poll.abort();
      signal.addEventListener("abort", abortPoll, { once: true });
      let hung = false;
      const ceiling = setTimeout(() => {
        hung = true;
        poll.abort();
      }, this.pollCeilingMs);
      try {
        const page = await this.fetchPage(operatorToken, cursor, poll.signal);
        if (signal.aborted) return;
        if (page.reset_required || page.events.length > 0) {
          // Worker and task changes can arrive several times during one provider
          // turn. The resulting snapshot is authoritative and includes every
          // change that lands during this short window, so refreshing it once
          // after the window avoids making the browser and API rebuild the same
          // roster repeatedly. Decisions, runtime/session state, presence and
          // notifications remain immediate because they can change available
          // operator actions or input safety.
          if (settlesBeforeRefresh(page)) {
            await this.delay(ORDINARY_INVALIDATION_SETTLE_MS, signal);
          }
          if (signal.aborted) return;
          await onInvalidate(page, poll.signal);
        }
        if (signal.aborted) return;
        if (poll.signal.aborted) throw new DOMException("Refresh deadline exceeded", "TimeoutError");
        cursor = page.next_cursor;
        failures = 0;
        onStateChange("connected");
      } catch (error) {
        if (signal.aborted) return;
        // A poll abandoned at the ceiling is a failure to retry, not the
        // caller stopping the feed.
        if (!hung && error instanceof DOMException && error.name === "AbortError") return;
        onStateChange("retrying");
        const delay = RETRY_DELAYS_MS[Math.min(failures, RETRY_DELAYS_MS.length - 1)];
        failures += 1;
        await this.delay(delay, signal).catch(() => undefined);
      } finally {
        clearTimeout(ceiling);
        signal.removeEventListener("abort", abortPoll);
      }
    }
  }
}

function settlesBeforeRefresh(page: ControlRoomEventPage): boolean {
  return !page.reset_required
    && page.events.length > 0
    && page.events.every(({ kind }) => kind === "tasks_changed" || kind === "workers_changed");
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
