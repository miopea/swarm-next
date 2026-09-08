type Sample = { at: number; fetch_elapsed_ms: number; resource_ms: number | null;
  request_to_first_byte_ms: number | null; body_transfer_ms: number | null;
  response_to_client_ms: number | null; server_handler_ms: number | null;
  server_auth_ms: number | null; server_capability_ms: number | null; server_validation_ms: number | null };
type Resource = Pick<PerformanceResourceTiming, "entryType" | "initiatorType" | "startTime" | "requestStart" | "responseStart" | "responseEnd">;
const WINDOW_MS = 3_600_000;

/** Local numeric evidence only. No URL, session, headers, content, observer or timer is retained. */
export class TerminalGrantEvidence {
  #samples: Sample[] = [];
  constructor(private readonly now: () => number = Date.now) {}

  record(start: number, end: number, entries: readonly Resource[], serverTiming: string | null = null): void {
    const at = this.now();
    if (![at, start, end].every(Number.isFinite) || at < 0 || start < 0 || end < start || end - start > WINDOW_MS) return;
    const candidates = entries.filter(entry => entry.entryType === "resource" && entry.initiatorType === "fetch"
      && [entry.startTime, entry.requestStart, entry.responseStart, entry.responseEnd].every(Number.isFinite)
      && entry.startTime >= start && entry.requestStart >= entry.startTime
      && entry.responseStart >= entry.requestStart && entry.responseEnd >= entry.responseStart
      && entry.responseEnd <= end && entry.responseEnd > 0);
    // Concurrent same-URL fetches or missing/restricted resource timing are unknown,
    // not a reason to pair this request with an older or arbitrary entry.
    const resource = candidates.length === 1 ? candidates[0] : undefined;
    this.#prune(at);
    this.#samples.push({ at, fetch_elapsed_ms: end - start,
      resource_ms: resource ? resource.responseEnd - resource.startTime : null,
      request_to_first_byte_ms: resource ? resource.responseStart - resource.requestStart : null,
      body_transfer_ms: resource ? resource.responseEnd - resource.responseStart : null,
      response_to_client_ms: resource ? end - resource.responseEnd : null,
      server_handler_ms: parseGrantServerTiming(serverTiming, end - start),
      server_auth_ms: parseGrantMetric(serverTiming, end - start, "swarm_grant_auth"),
      server_capability_ms: parseGrantMetric(serverTiming, end - start, "swarm_grant_capability"),
      server_validation_ms: parseGrantMetric(serverTiming, end - start, "swarm_grant_validation") });
    if (this.#samples.length > 200) this.#samples.shift();
  }

  snapshot() {
    this.#prune(this.now());
    const slowest = this.#samples.reduce<Sample | undefined>((old, current) =>
      !old || current.fetch_elapsed_ms > old.fetch_elapsed_ms ? current : old, undefined);
    return { samples: this.#samples.length, matched_resource_samples: this.#samples.filter(s => s.resource_ms !== null).length,
      slowest: slowest ? { ...slowest } : null };
  }

  #prune(at: number): void {
    this.#samples = this.#samples.filter(sample => sample.at <= at && sample.at >= at - WINDOW_MS);
  }
}

export const terminalGrantEvidence = new TerminalGrantEvidence();

/** Resource entries are read only after the response body completes; collection cannot break attachment. */
export function recordTerminalGrantRequest(url: string, start: number, end: number, serverTiming: string | null): void {
  let entries: PerformanceResourceTiming[] = [];
  try { entries = performance.getEntriesByName(url, "resource") as PerformanceResourceTiming[]; }
  catch { /* Unsupported, restricted or absent timing remains unknown. */ }
  terminalGrantEvidence.record(start, end, entries, serverTiming);
}

/** Accept only Swarm's bounded numeric metric; never retain arbitrary header text. */
export function parseGrantServerTiming(value: string | null, elapsed: number): number | null {
  return parseGrantMetric(value, elapsed, "swarm_grant");
}

function parseGrantMetric(value: string | null, elapsed: number,
  metric: "swarm_grant" | "swarm_grant_auth" | "swarm_grant_capability" | "swarm_grant_validation"): number | null {
  if (typeof value !== "string" || !value || value.length > 1024 || !Number.isFinite(elapsed) || elapsed < 0) return null;
  const metrics = value.split(",").filter(part => part.split(";", 1)[0].trim() === metric);
  if (metrics.length !== 1) return null;
  const match = new RegExp(`^\\s*${metric};dur=(\\d+(?:\\.\\d+)?)\\s*$`).exec(metrics[0]);
  if (!match) return null;
  const duration = Number(match[1]);
  // Independent monotonic clocks can round differently at sub-millisecond scale.
  return Number.isFinite(duration) && duration <= 60_000 && duration <= elapsed + 1 ? duration : null;
}
