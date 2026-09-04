/** Content-free, browser-lifetime experiment evidence. No worker IDs or output. */
export class TerminalRestoreEvidence {
  readonly #now: () => number;
  #generation = 0;
  #started = 0;
  #pending = 0;
  #interrupted = 0;
  #failed = 0;
  #samples: { at: number; ms: number }[] = [];

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

  begin(): (outcome: "rendered" | "interrupted" | "failed") => void {
    const generation = this.#generation;
    const startedAt = this.#now();
    let settled = false;
    this.#started += 1;
    this.#pending += 1;
    return (outcome) => {
      if (settled || generation !== this.#generation) return;
      settled = true;
      this.#pending -= 1;
      if (outcome === "interrupted") { this.#interrupted += 1; return; }
      if (outcome === "failed") { this.#failed += 1; return; }
      const at = this.#now();
      const ms = at - startedAt;
      if (!Number.isFinite(ms) || ms < 0) { this.#failed += 1; return; }
      this.#prune(at);
      this.#samples.push({ at, ms });
      if (this.#samples.length > 200) this.#samples.shift();
    };
  }

  snapshot() {
    this.#prune(this.#now());
    const values = this.#samples.map((sample) => sample.ms).sort((left, right) => left - right);
    return {
      started: this.#started, pending: this.#pending, interrupted: this.#interrupted, failed: this.#failed,
      samples: values.length,
      p95_ms: values.length ? values[Math.ceil(values.length * 0.95) - 1] : null,
      max_ms: values.length ? values[values.length - 1] : null,
    };
  }

  #prune(now: number): void {
    this.#samples = this.#samples.filter((sample) => sample.at >= now - 60 * 60_000 && sample.at <= now);
  }
}
