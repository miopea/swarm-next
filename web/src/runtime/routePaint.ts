export type RoutePaintSample = {
  /** Which workspace section was opened. */
  surface: string;
  /** Milliseconds from the route change to the first frame after it painted. */
  duration_ms: number;
  observed_at: number;
};

export type RoutePaintSummary = {
  samples: number;
  slowest_ms: number;
  median_ms: number;
};

const STORAGE_KEY = "swarm-next.route-paint.v1";
const MAX_SAMPLES = 20;

/**
 * Records how long a workspace section took to reach the screen.
 *
 * The operator reports the previous workspace lingering after a route change,
 * and an automated phone-sized proof saw captured pixels keep the old surface
 * for a second or two while the accessibility tree had already moved. Neither
 * observation says how long it actually takes, and the standing instruction is
 * to measure that before reaching for another redraw workaround — a delay
 * added to hide it would be exactly the kind of timing-as-correctness this
 * architecture rules out.
 */
export function recordRoutePaint(surface: string, durationMs: number, now = Date.now()) {
  const sample: RoutePaintSample = {
    surface,
    duration_ms: Math.round(durationMs),
    observed_at: now,
  };
  try {
    window.sessionStorage.setItem(
      STORAGE_KEY,
      JSON.stringify([...readRoutePaints(), sample].slice(-MAX_SAMPLES)),
    );
  } catch {
    // Measurement must never be load-bearing: a browser refusing storage keeps
    // the product working and simply reports nothing.
  }
}

export function readRoutePaints(): RoutePaintSample[] {
  try {
    const parsed = JSON.parse(window.sessionStorage.getItem(STORAGE_KEY) ?? "[]") as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isRoutePaintSample).slice(-MAX_SAMPLES);
  } catch {
    return [];
  }
}

/**
 * Reduces the samples to what a reader can act on: how many were seen, the
 * worst, and the middle. The worst matters because the complaint is about the
 * occasional slow one, and a mean would bury it.
 */
export function routePaintSummary(samples: RoutePaintSample[]): RoutePaintSummary | undefined {
  if (samples.length === 0) return undefined;
  const durations = samples.map((sample) => sample.duration_ms).sort((left, right) => left - right);
  const middle = Math.floor(durations.length / 2);
  const median = durations.length % 2 === 0
    ? Math.round((durations[middle - 1] + durations[middle]) / 2)
    : durations[middle];
  return {
    samples: durations.length,
    slowest_ms: durations[durations.length - 1],
    median_ms: median,
  };
}

/**
 * Measures one route change, from the moment it is requested to the first frame
 * after the browser has painted it.
 *
 * A single animation frame runs *before* that frame is painted, so the callback
 * is deferred one further frame: when the second fires, the frame carrying the
 * new surface has been presented. This is the closest a browser lets an
 * application observe its own paint for a client-side route change.
 *
 * Returns a cancel function, so a route abandoned before it paints records
 * nothing rather than attributing the next surface's time to it.
 */
export function measureRoutePaint(
  surface: string,
  schedule: (callback: () => void) => number,
  cancel: (handle: number) => void,
  clock: () => number = () => performance.now(),
  record: (surface: string, durationMs: number) => void = recordRoutePaint,
): () => void {
  const startedAt = clock();
  let inner: number | undefined;
  const outer = schedule(() => {
    inner = schedule(() => record(surface, clock() - startedAt));
  });
  return () => {
    cancel(outer);
    if (inner !== undefined) cancel(inner);
  };
}

function isRoutePaintSample(value: unknown): value is RoutePaintSample {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<RoutePaintSample>;
  return typeof candidate.surface === "string"
    && typeof candidate.duration_ms === "number"
    && typeof candidate.observed_at === "number";
}
