/** Browser-owned, content-free evidence. Never pass input or terminal bytes here. */
export const BROWSER_METRICS = ["long_task", "interaction", "route", "terminal_render", "terminal_reconnect"] as const;
export type BrowserMetric = typeof BROWSER_METRICS[number];
type Aggregate = { count: number; total_ms: number; max_ms: number };
type Bucket = { at: number; metrics: Partial<Record<BrowserMetric, Aggregate>> };
type Incident = { at: number; until: number; trigger: BrowserMetric; severity: "slow" | "critical"; buckets: Bucket[] };
const BUCKET_MS = 10_000;
const WINDOW_MS = 60 * 60_000;
const EXPIRY_MS = 24 * WINDOW_MS;
const STORAGE_KEY = "swarm.browser-performance.v1";
const MAX_STORED_CHARS = 500_000;

function validMetric(value: unknown): value is BrowserMetric {
  return typeof value === "string" && (BROWSER_METRICS as readonly string[]).includes(value);
}

function copyBucket(bucket: Bucket): Bucket {
  return { at: bucket.at, metrics: Object.fromEntries(Object.entries(bucket.metrics).map(([key, value]) => [key, { ...value }])) };
}

export class BrowserPerformanceRecorder {
  #buckets: Bucket[] = [];
  #incidents: Incident[] = [];
  constructor(private readonly now: () => number = Date.now) {}

  record(metric: BrowserMetric, durationMs: number): void {
    if (!validMetric(metric) || !Number.isFinite(durationMs) || durationMs < 0 || durationMs > EXPIRY_MS) return;
    const now = this.now();
    const at = Math.floor(now / BUCKET_MS) * BUCKET_MS;
    let bucket = this.#buckets.at(-1);
    if (!bucket || bucket.at !== at) {
      this.#prune(now);
      bucket = { at, metrics: {} };
      this.#buckets.push(bucket);
      this.#buckets = this.#buckets.slice(-360);
    }
    const old = bucket.metrics[metric] ?? { count: 0, total_ms: 0, max_ms: 0 };
    // Saturate rather than let adversarial event volume overflow the counters.
    if (old.count < 1_000_000) {
      bucket.metrics[metric] = { count: old.count + 1, total_ms: old.total_ms + Math.round(durationMs), max_ms: Math.max(old.max_ms, Math.round(durationMs)) };
    }
    const threshold = metric === "terminal_reconnect" ? 2_000 : 1_000;
    const repeatedBlocking = metric === "long_task" && (bucket.metrics.long_task?.total_ms ?? 0) > 1_000;
    if ((durationMs > threshold || repeatedBlocking) && !this.#incidents.some((incident) => now <= incident.until)) {
      this.#incidents.push({ at: now, until: now + 60_000, trigger: metric, severity: durationMs > 3_000 ? "critical" : "slow", buckets: this.#buckets.filter((item) => item.at >= at - 120_000).map(copyBucket) });
      this.#incidents = this.#incidents.slice(-5);
    }
    for (const incident of this.#incidents) {
      if (now > incident.until) continue;
      if (durationMs > 3_000) incident.severity = "critical";
      const last = incident.buckets.at(-1);
      if (last?.at === bucket.at) incident.buckets[incident.buckets.length - 1] = copyBucket(bucket);
      else incident.buckets.push(copyBucket(bucket));
      incident.buckets = incident.buckets.slice(-20);
    }
  }

  snapshot() {
    this.#prune(this.now());
    return {
      schema: 1 as const,
      captured_at: this.now(),
      buckets: this.#buckets.map(copyBucket),
      incidents: this.#incidents.map((incident) => ({ ...incident, buckets: incident.buckets.map(copyBucket) })),
    };
  }

  #prune(now: number) {
    this.#buckets = this.#buckets.filter((bucket) => bucket.at >= now - WINDOW_MS && bucket.at <= now);
    this.#incidents = this.#incidents.filter((incident) => incident.at >= now - EXPIRY_MS && incident.at <= now);
  }
}

export const browserPerformance = new BrowserPerformanceRecorder();
type Snapshot = ReturnType<BrowserPerformanceRecorder["snapshot"]>;

/** Project untrusted storage onto the numeric schema; never echo arbitrary fields. */
export function readPreviousBrowserPerformance(storage: Pick<Storage, "getItem">, now = Date.now()): Snapshot | undefined {
  try {
    const raw = storage.getItem(STORAGE_KEY);
    if (!raw || raw.length > MAX_STORED_CHARS) return undefined;
    const parsed = JSON.parse(raw);
    if (parsed?.schema !== 1 || !Number.isFinite(parsed.captured_at) || parsed.captured_at < now - EXPIRY_MS || parsed.captured_at > now) return undefined;
    const buckets = (input: unknown): Bucket[] => {
      if (!Array.isArray(input)) return [];
      return input.slice(-360).flatMap((item) => {
        if (!item || !Number.isFinite(item.at) || item.at < now - EXPIRY_MS || item.at > now) return [];
        const metrics: Bucket["metrics"] = {};
        for (const metric of BROWSER_METRICS) {
          const value = item.metrics?.[metric];
          if (!value || !Number.isInteger(value.count) || value.count < 1 || value.count > 1_000_000
            || !Number.isFinite(value.total_ms) || value.total_ms < 0 || value.total_ms > EXPIRY_MS * value.count
            || !Number.isFinite(value.max_ms) || value.max_ms < 0 || value.max_ms > EXPIRY_MS) continue;
          metrics[metric] = { count: value.count, total_ms: value.total_ms, max_ms: value.max_ms };
        }
        return [{ at: item.at, metrics }];
      });
    };
    return {
      schema: 1, captured_at: parsed.captured_at, buckets: buckets(parsed.buckets),
      incidents: (Array.isArray(parsed.incidents) ? parsed.incidents : []).slice(-5).flatMap((item: Partial<Incident>) => {
        if (!Number.isFinite(item.at) || !Number.isFinite(item.until) || !validMetric(item.trigger)
          || item.at! < now - EXPIRY_MS || item.at! > now || item.until !== item.at! + 60_000
          || (item.severity !== "slow" && item.severity !== "critical")) return [];
        return [{ at: item.at!, until: item.until, trigger: item.trigger, severity: item.severity, buckets: buckets(item.buckets).slice(-20) }];
      }),
    };
  } catch { return undefined; }
}

export function saveBrowserPerformance(storage: Pick<Storage, "setItem">, recorder = browserPerformance): void {
  try { storage.setItem(STORAGE_KEY, JSON.stringify(recorder.snapshot())); } catch { /* Storage is optional. */ }
}

let previous: Snapshot | undefined;
let observed: string[] = [];
let installed = false;

/** One application owner; no sampling timers, DOM walks, or per-event storage writes. */
export function installBrowserPerformanceCapture(): () => void {
  if (installed) return () => undefined;
  installed = true;
  try { previous = readPreviousBrowserPerformance(window.sessionStorage); } catch { previous = undefined; }
  const observers: PerformanceObserver[] = [];
  observed = [];
  if (typeof PerformanceObserver !== "undefined") {
    for (const type of ["longtask", "event"]) {
      if (!PerformanceObserver.supportedEntryTypes?.includes(type)) continue;
      let observer: PerformanceObserver | undefined;
      try {
        observer = new PerformanceObserver((list) => {
          if (document.visibilityState !== "visible") return;
          for (const entry of list.getEntries()) {
            // Event Timing names and targets may expose content; retain duration only.
            browserPerformance.record(type === "longtask" ? "long_task" : "interaction", entry.duration);
          }
        });
        observer.observe({ type, buffered: false });
        observers.push(observer);
        observed.push(type);
      } catch { observer?.disconnect(); }
    }
  }
  const persist = () => { try { saveBrowserPerformance(window.sessionStorage); } catch { /* Optional storage. */ } };
  window.addEventListener("pagehide", persist);
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    observers.forEach((observer) => observer.disconnect());
    window.removeEventListener("pagehide", persist);
    persist();
    installed = false;
    observed = [];
  };
}

export function readBrowserPerformance() {
  if (previous && Date.now() - previous.captured_at > EXPIRY_MS) previous = undefined;
  return { collection: installed ? "active" : "not_installed", supported_observers: [...observed], current: browserPerformance.snapshot(), before_reload: previous };
}
