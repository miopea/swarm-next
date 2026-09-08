export type PageActivity = "foreground" | "unfocused" | "hidden" | "unknown";
export type ObservationActivity = PageActivity | "changed";

/** Monotonic, content-free local context. No polling or durable storage. */
export class PageActivityEvidence {
  #changes: { at: number; state: PageActivity }[] = [];
  constructor(private readonly now: () => number = () => performance.now()) {}

  record(state: PageActivity): void {
    const at = this.now();
    if (!Number.isFinite(at) || at < 0) return;
    if (at < (this.#changes.at(-1)?.at ?? 0)) this.#changes = [];
    if (this.#changes.at(-1)?.state !== state) this.#changes.push({ at, state });
    this.#prune(at);
    this.#changes = this.#changes.slice(-200);
  }

  during(start: number, duration: number): ObservationActivity {
    const now = this.now();
    this.#prune(now);
    const end = start + duration;
    if (![start, duration, now, end].every(Number.isFinite) || start < 0 || duration < 0
      || duration > 60_000 || start < now - 60_000 || end > now) return "unknown";
    const before = this.#changes.filter(change => change.at <= start).at(-1);
    if (!before) return "unknown";
    const within = this.#changes.filter(change => change.at > start && change.at <= end);
    if (before.state === "unknown" || within.some(change => change.state === "unknown")) return "unknown";
    return within.length ? "changed" : before.state;
  }

  #prune(now: number): void {
    // Retain one boundary preceding the window, not an unbounded transition log.
    const firstRecent = this.#changes.findIndex(change => change.at >= now - 60_000);
    if (firstRecent > 0) this.#changes = this.#changes.slice(firstRecent - 1);
    else if (firstRecent === -1) this.#changes = this.#changes.slice(-1);
    this.#changes = this.#changes.filter(change => change.at <= now);
  }
}

export function currentPageActivity(): PageActivity {
  try {
    if (document.visibilityState === "hidden") return "hidden";
    if (document.visibilityState !== "visible" || typeof document.hasFocus !== "function") return "unknown";
    return document.hasFocus() ? "foreground" : "unfocused";
  } catch { return "unknown"; }
}
