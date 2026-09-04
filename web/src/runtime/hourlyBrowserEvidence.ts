import type { BrowserEvidenceHour } from "../api";
import { BROWSER_METRICS, type BrowserMetric } from "./browserPerformance";

const HOUR_MS = 3_600_000;
const MAX_PENDING = 24;
const MAX_AGE_MS = 24 * HOUR_MS;
const MAX_SAMPLES = 1_000_000;

/** One collector/build lifetime; retries return the exact capture identity. */
export class HourlyBrowserEvidence {
  #pending: BrowserEvidenceHour[] = [];
  #acknowledged = new Map<string, { revision: number; samples: number }>();
  #current: BrowserEvidenceHour | undefined;
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
    let capture = this.#current;
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
      this.#current = capture;
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
    const capture = this.#pending.find((item) => this.#acknowledged.get(item.capture_id)?.revision !== item.revision);
    return capture ? structuredClone(capture) : undefined;
  }

  acknowledge(capture: BrowserEvidenceHour): void {
    const current = this.#pending.find((item) => item.capture_id === capture.capture_id);
    if (!current || current.build !== capture.build || current.hour !== capture.hour
      || capture.revision > current.revision) return;
    if (capture.revision > (this.#acknowledged.get(capture.capture_id)?.revision ?? 0)) {
      this.#acknowledged.set(capture.capture_id, { revision: capture.revision, samples: sampleCount(capture) });
    }
  }

  serialize(now: number): string {
    this.#prune(now);
    return JSON.stringify({ schema: 1, saved_at: now, acknowledged: Object.fromEntries(this.#acknowledged),
      captures: this.#pending.filter((item) => this.#acknowledged.get(item.capture_id)?.revision !== item.revision) });
  }

  /** Restored captures are retry-only; new samples get a new identity, even in the same hour. */
  restore(raw: string | null, now: number): boolean {
    if (!raw) return true;
    if (raw.length > 65_536 || this.#pending.length) return false;
    try {
      const parsed = JSON.parse(raw);
      if (parsed.schema !== 1 || !Number.isSafeInteger(parsed.saved_at)
        || parsed.saved_at > now || parsed.saved_at < now - MAX_AGE_MS
        || !Array.isArray(parsed.captures) || parsed.captures.length > MAX_PENDING) return false;
      const captures = parsed.captures.map((value: unknown) => validateCapture(value, now));
      if (captures.some((value: BrowserEvidenceHour | undefined) => !value)
        || new Set(captures.map((value: BrowserEvidenceHour) => value.capture_id)).size !== captures.length) return false;
      this.#pending = captures;
      for (const capture of this.#pending) {
        const ack = parsed.acknowledged?.[capture.capture_id];
        if (ack && Number.isSafeInteger(ack.revision) && ack.revision > 0 && ack.revision < capture.revision
          && Number.isSafeInteger(ack.samples) && ack.samples >= 0 && ack.samples <= sampleCount(capture)) {
          this.#acknowledged.set(capture.capture_id, { revision: ack.revision, samples: ack.samples });
        }
      }
      return true;
    } catch { return false; }
  }

  get status() { return { retained: this.#pending.length, dropped_samples: this.#lost }; }

  #prune(now: number): void {
    const keep = this.#pending.filter((item) => item.hour * 1000 >= now - MAX_AGE_MS).slice(-MAX_PENDING);
    for (const item of this.#pending) {
      if (keep.includes(item)) continue;
      this.#lost += sampleCount(item) - (this.#acknowledged.get(item.capture_id)?.samples ?? 0);
      this.#acknowledged.delete(item.capture_id);
    }
    this.#pending = keep;
    if (this.#current && !keep.includes(this.#current)) this.#current = undefined;
  }
}

function sampleCount(capture: BrowserEvidenceHour): number {
  return BROWSER_METRICS.reduce((sum, metric) => sum + capture[metric].count, 0);
}

function validateCapture(input: unknown, now: number): BrowserEvidenceHour | undefined {
  if (!input || typeof input !== "object") return undefined;
  const value = input as BrowserEvidenceHour;
  if (typeof value.capture_id !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value.capture_id)
    || value.capture_id === "00000000-0000-0000-0000-000000000000"
    || typeof value.build !== "string" || !/^[a-zA-Z0-9.+_-]{1,128}$/.test(value.build)
    || !Number.isSafeInteger(value.hour) || value.hour < 0 || value.hour % 3600 !== 0
    || value.hour * 1000 > now || value.hour * 1000 < now - MAX_AGE_MS
    || !Number.isSafeInteger(value.revision) || value.revision < 1 || value.revision > 0xffff_ffff) return undefined;
  const result = { capture_id: value.capture_id, build: value.build, hour: value.hour, revision: value.revision } as BrowserEvidenceHour;
  for (const metric of BROWSER_METRICS) {
    const timing = value[metric];
    if (!timing || !Number.isSafeInteger(timing.count) || timing.count < 0 || timing.count > MAX_SAMPLES
      || !Number.isSafeInteger(timing.total_ms) || timing.total_ms < 0
      || !Number.isSafeInteger(timing.max_ms) || timing.max_ms < 0 || timing.max_ms > MAX_AGE_MS
      || timing.total_ms < timing.max_ms || timing.total_ms > timing.count * timing.max_ms
      || (timing.count === 0 && timing.max_ms !== 0)) return undefined;
    result[metric] = { count: timing.count, total_ms: timing.total_ms, max_ms: timing.max_ms };
  }
  return result;
}
