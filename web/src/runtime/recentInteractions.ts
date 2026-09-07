/** Numeric-only, local evidence. No event names, targets, input or persisted IDs. */
type Entry = { interactionId?: number; duration: number; startTime: number; processingStart?: number; processingEnd?: number };
type Timing = { duration_ms: number; input_delay_ms: number; processing_ms: number; presentation_estimate_ms: number };
const WINDOW_MS = 60_000;
const MAX_INTERACTIONS = 200;

export class RecentInteractions {
  #entries = new Map<number, { at: number; timing: Timing }>();
  #unattributed: { at: number; timing: Timing }[] = [];
  constructor(private readonly now: () => number = Date.now) {}

  record(entry: Entry): void {
    const { interactionId: id, duration, startTime, processingStart: start, processingEnd: end } = entry;
    if ((id !== undefined && (!Number.isSafeInteger(id) || id < 0)) || !Number.isFinite(duration) || duration < 0 || duration > WINDOW_MS
      || !Number.isFinite(startTime) || startTime < 0 || !Number.isFinite(start) || !Number.isFinite(end)
      || start! < startTime || end! < start! || end! - startTime > WINDOW_MS) return;
    const now = this.now();
    this.#prune(now);
    // Zero/missing IDs are native event entries, not identified interactions.
    // Keep their numeric phases separately so raw-event delays remain explainable.
    if (id === undefined || id === 0) {
      this.#unattributed.push({ at: now, timing: {
        duration_ms: duration, input_delay_ms: start! - startTime,
        processing_ms: end! - start!,
        presentation_estimate_ms: Math.max(0, duration - (end! - startTime)),
      } });
      if (this.#unattributed.length > MAX_INTERACTIONS) this.#unattributed.shift();
      return;
    }
    const prior = this.#entries.get(id!);
    // All phases belong to the same slowest entry, not independently selected maxima.
    const timing = !prior || duration > prior.timing.duration_ms ? {
      duration_ms: duration,
      input_delay_ms: start! - startTime,
      processing_ms: end! - start!,
      // Native duration is quantized; a small negative remainder is not negative paint time.
      presentation_estimate_ms: Math.max(0, duration - (end! - startTime)),
    } : prior.timing;
    this.#entries.delete(id!);
    this.#entries.set(id!, { at: now, timing });
    if (this.#entries.size > MAX_INTERACTIONS) this.#entries.delete(this.#entries.keys().next().value!);
  }

  snapshot() {
    this.#prune(this.now());
    const timings = [...this.#entries.values()].map((entry) => entry.timing);
    const slowest = timings.reduce<Timing | undefined>((worst, timing) => !worst || timing.duration_ms > worst.duration_ms ? timing : worst, undefined);
    const unattributed = this.#unattributed.reduce<Timing | undefined>((worst, entry) => !worst || entry.timing.duration_ms > worst.duration_ms ? entry.timing : worst, undefined);
    return {
      window_ms: WINDOW_MS, retained_limit: MAX_INTERACTIONS, observed_interactions: timings.length,
      slowest: slowest ? { ...slowest } : null,
      unattributed_event_entries: this.#unattributed.length,
      slowest_unattributed: unattributed ? { ...unattributed } : null,
      coverage: "Only native Event Timing entries above the browser's reporting threshold; not all interactions or page INP. Phases describe the slowest retained entry. Presentation is an estimate from quantized duration. IDs are discarded from reports and on reload.",
    };
  }

  #prune(now: number) {
    for (const [id, entry] of this.#entries) if (entry.at < now - WINDOW_MS || entry.at > now) this.#entries.delete(id);
    this.#unattributed = this.#unattributed.filter((entry) => entry.at >= now - WINDOW_MS && entry.at <= now);
  }
}
