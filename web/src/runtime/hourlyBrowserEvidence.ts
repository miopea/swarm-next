import type { BrowserEvidenceHour } from "../api";
import { BROWSER_METRICS, type BrowserMetric } from "./browserPerformance";

const HOUR_MS = 3_600_000;
const MAX_PENDING = 24;
const MAX_AGE_MS = 24 * HOUR_MS;
const MAX_SAMPLES = 1_000_000;

/** One collector/build lifetime; retries return the exact capture identity. */
export class HourlyBrowserEvidence {
  #pending: BrowserEvidenceHour[] = [];
  #acknowledged = new Map<string, number>();
  #lost = 0;
  #lastTime = 0;

  constructor(readonly build: string, private readonly uuid: () => string = () => crypto.randomUUID()) {
    if (!/^[a-zA-Z0-9.+_-]{1,128}$/.test(build)) throw new Error("Invalid evidence build");
  }

  record(metric: BrowserMetric, duration: number, at: number): void {
    if (!BROWSER_METRICS.includes(metric) || !Number.isSafeInteger(duration) || duration < 0
      || duration > MAX_AGE_MS || !Number.isSafeInteger(at) || at < this.#lastTime) {
      this.#lost++;
      return;
    }
    this.#lastTime = at;
    this.#prune(at);
    const hour = Math.floor(at / HOUR_MS) * 3600;
    let capture = this.#pending.at(-1);
    if (!capture || capture.hour !== hour) {
      let captureId: string;
      try { captureId = this.uuid(); } catch { this.#lost++; return; }
      capture = {
        capture_id: captureId, build: this.build, hour, revision: 1,
        long_task: { count: 0, total_ms: 0, max_ms: 0 },
        interaction: { count: 0, total_ms: 0, max_ms: 0 },
        route: { count: 0, total_ms: 0, max_ms: 0 },
        terminal_render: { count: 0, total_ms: 0, max_ms: 0 },
        terminal_reconnect: { count: 0, total_ms: 0, max_ms: 0 },
      };
      this.#pending.push(capture);
      this.#prune(at);
    }
    const timing = capture[metric];
    if (timing.count >= MAX_SAMPLES) { this.#lost++; return; }
    timing.count++;
    timing.total_ms += duration;
    timing.max_ms = Math.max(timing.max_ms, duration);
    capture.revision++;
  }

  /** Oldest dirty capture first; caller owns one bounded in-flight request. */
  next(now: number): BrowserEvidenceHour | undefined {
    this.#prune(now);
    const capture = this.#pending.find((item) => this.#acknowledged.get(item.capture_id) !== item.revision);
    return capture ? structuredClone(capture) : undefined;
  }

  acknowledge(capture: BrowserEvidenceHour): void {
    const current = this.#pending.find((item) => item.capture_id === capture.capture_id);
    if (!current || current.build !== capture.build || current.hour !== capture.hour
      || capture.revision > current.revision) return;
    this.#acknowledged.set(capture.capture_id,
      Math.max(capture.revision, this.#acknowledged.get(capture.capture_id) ?? 0));
  }

  get status() { return { retained: this.#pending.length, dropped_samples: this.#lost }; }

  #prune(now: number): void {
    const keep = this.#pending.filter((item) => item.hour * 1000 >= now - MAX_AGE_MS).slice(-MAX_PENDING);
    for (const item of this.#pending) {
      if (keep.includes(item)) continue;
      if (this.#acknowledged.get(item.capture_id) !== item.revision) {
        this.#lost += BROWSER_METRICS.reduce((sum, metric) => sum + item[metric].count, 0);
      }
      this.#acknowledged.delete(item.capture_id);
    }
    this.#pending = keep;
  }
}
