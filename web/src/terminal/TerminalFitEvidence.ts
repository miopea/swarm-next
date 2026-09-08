import type { FitMilestone } from "./TerminalRestoreEvidence";

type FitSample = { at: number; initial: boolean; failed: boolean; total_ms: number; font_ms: number | null; after_fonts_ms: number | null; frames: number; max_frame_gap_ms: number };

/** Bounded local elapsed timings, not CPU measurements. No identities or text. */
export class TerminalFitEvidence {
  #samples: FitSample[] = [];
  constructor(private readonly now: () => number = () => performance.now()) {}

  begin(initial: boolean) {
    const started = this.now();
    let fonts: number | undefined;
    let previousFrame: number | undefined;
    let frames = 0;
    let maxGap = 0;
    let finished = false;
    return {
      milestone: (phase: FitMilestone) => {
        if (finished) return;
        const at = this.now();
        if (phase === "fonts_ready") fonts ??= at;
        if (phase === "fit_frame" && fonts !== undefined) {
          const previous = previousFrame ?? fonts;
          if (Number.isFinite(at) && at >= previous) {
            frames += 1;
            maxGap = Math.max(maxGap, at - previous);
            previousFrame = at;
          }
        }
      },
      finish: (failed: boolean) => {
        if (finished) return;
        finished = true;
        const at = this.now();
        const elapsed = at - started;
        if (![started, at, elapsed].every(Number.isFinite) || elapsed < 0 || elapsed > 3_600_000) return;
        const phased = fonts !== undefined && Number.isFinite(fonts) && fonts >= started && fonts <= at;
        this.#prune(at);
        this.#samples.push({ at, initial, failed, total_ms: elapsed,
          font_ms: phased ? fonts! - started : null, after_fonts_ms: phased ? at - fonts! : null,
          frames, max_frame_gap_ms: maxGap });
        if (this.#samples.length > 200) this.#samples.shift();
      },
    };
  }

  snapshot() {
    this.#prune(this.now());
    const followups = this.#samples.filter(sample => !sample.initial);
    const slowest = followups.reduce<FitSample | undefined>((worst, sample) => !worst || sample.total_ms > worst.total_ms ? sample : worst, undefined);
    return { samples: followups.length, failed: followups.filter(sample => sample.failed).length, slowest: slowest ? { ...slowest } : null };
  }

  #prune(at: number) {
    this.#samples = this.#samples.filter(sample => sample.at <= at && sample.at >= at - 3_600_000);
  }
}

export const terminalFitEvidence = new TerminalFitEvidence();
