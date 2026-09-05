export type FitMilestone = "fit_started" | "fonts_ready" | "fit_frame";
type FitPhases = { opening_ms: number; font_ms: number; layout_ms: number; frame_count?: number; max_frame_gap_ms?: number };
type RestoreSample = { at: number; ms: number; setup_ms: number | null; connection_ms: number | null; fit: FitPhases | null };

/** Content-free, browser-lifetime experiment evidence. No worker IDs or output. */
export class TerminalRestoreEvidence {
  readonly #now: () => number;
  #generation = 0;
  #started = 0;
  #pending = 0;
  #interrupted = 0;
  #failed = 0;
  #samples: RestoreSample[] = [];

  constructor(now: () => number = () => performance.now()) { this.#now = now; }

  reset(): void {
    this.#generation += 1;
    this.#started = this.#pending = this.#interrupted = this.#failed = 0;
    this.#samples = [];
  }

  stop(): void {
    this.#generation += 1;
    this.#interrupted += this.#pending;
    this.#pending = 0;
  }

  begin(): (outcome: FitMilestone | "connection_started" | "rendered" | "interrupted" | "failed") => void {
    const generation = this.#generation;
    const startedAt = this.#now();
    let settled = false;
    let connectionAt: number | undefined;
    let fitAt: number | undefined;
    let fontsAt: number | undefined;
    let frameCount = 0;
    let previousFrameAt: number | undefined;
    let maxFrameGap = 0;
    this.#started += 1;
    this.#pending += 1;
    return (outcome) => {
      if (settled || generation !== this.#generation) return;
      if (outcome === "fit_started") { fitAt ??= this.#now(); return; }
      if (outcome === "fonts_ready") { fontsAt ??= this.#now(); return; }
      if (outcome === "fit_frame") {
        const at = this.#now();
        const previous = previousFrameAt ?? fontsAt;
        if (previous !== undefined && Number.isFinite(at) && at >= previous) {
          frameCount += 1;
          maxFrameGap = Math.max(maxFrameGap, at - previous);
          previousFrameAt = at;
        }
        return;
      }
      if (outcome === "connection_started") {
        connectionAt ??= this.#now();
        return;
      }
      settled = true;
      this.#pending -= 1;
      if (outcome === "interrupted") { this.#interrupted += 1; return; }
      if (outcome === "failed") { this.#failed += 1; return; }
      const at = this.#now();
      const ms = at - startedAt;
      if (!Number.isFinite(ms) || ms < 0) { this.#failed += 1; return; }
      this.#prune(at);
      const phased = connectionAt !== undefined && Number.isFinite(connectionAt) && connectionAt >= startedAt && connectionAt <= at;
      const fitPhased = phased && fitAt !== undefined && fontsAt !== undefined
        && Number.isFinite(fitAt) && Number.isFinite(fontsAt)
        && startedAt <= fitAt && fitAt <= fontsAt && fontsAt <= connectionAt!;
      this.#samples.push({ at, ms, setup_ms: phased ? connectionAt! - startedAt : null, connection_ms: phased ? at - connectionAt! : null,
        fit: fitPhased ? { opening_ms: fitAt! - startedAt, font_ms: fontsAt! - fitAt!, layout_ms: connectionAt! - fontsAt!,
          ...(frameCount ? { frame_count: frameCount, max_frame_gap_ms: maxFrameGap } : {}) } : null });
      if (this.#samples.length > 200) this.#samples.shift();
    };
  }

  snapshot() {
    this.#prune(this.#now());
    const values = this.#samples.map((sample) => sample.ms).sort((left, right) => left - right);
    const slowest = this.#samples.reduce<RestoreSample | undefined>((worst, sample) => !worst || sample.ms > worst.ms ? sample : worst, undefined);
    return {
      started: this.#started, pending: this.#pending, interrupted: this.#interrupted, failed: this.#failed,
      samples: values.length,
      p95_ms: values.length ? values[Math.ceil(values.length * 0.95) - 1] : null,
      max_ms: values.length ? values[values.length - 1] : null,
      slowest: slowest ? { total_ms: slowest.ms, setup_ms: slowest.setup_ms, connection_ms: slowest.connection_ms } : null,
      slowest_fit: slowest?.fit ? { ...slowest.fit } : null,
    };
  }

  #prune(now: number): void {
    this.#samples = this.#samples.filter((sample) => sample.at >= now - 60 * 60_000 && sample.at <= now);
  }
}
