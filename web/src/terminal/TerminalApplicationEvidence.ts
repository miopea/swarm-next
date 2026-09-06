type Sample = { at: number; total_ms: number; state_ms: number; geometry_ms: number; bytes: number };

/** Completed snapshot applications only. No session identity, text, or timers. */
export class TerminalApplicationEvidence {
  #samples: Sample[] = [];
  constructor(private readonly now: () => number = Date.now) {}

  record(bytes: number, stateMs: number, geometryMs: number): void {
    if (!Number.isSafeInteger(bytes) || bytes < 0 || bytes > 3 * 1024 * 1024
      || ![stateMs, geometryMs].every(value => Number.isFinite(value) && value >= 0 && value <= 3_600_000)) return;
    const at = this.now();
    if (!Number.isFinite(at)) return;
    this.#prune(at);
    this.#samples.push({ at, bytes, state_ms: stateMs, geometry_ms: geometryMs, total_ms: stateMs + geometryMs });
    if (this.#samples.length > 200) this.#samples.shift();
  }

  snapshot() {
    this.#prune(this.now());
    const slowest = this.#samples.reduce<Sample | undefined>((previous, current) =>
      !previous || current.total_ms > previous.total_ms ? current : previous, undefined);
    return { samples: this.#samples.length, slowest: slowest ? { ...slowest } : null };
  }

  #prune(at: number): void {
    this.#samples = this.#samples.filter(sample => sample.at <= at && sample.at >= at - 3_600_000);
  }
}

export const terminalApplicationEvidence = new TerminalApplicationEvidence();
