import { fetchDevelopmentRuntime, fetchHealth, RuntimeRequestError, type DevelopmentRuntime } from "../api";

export type DevelopmentBuildObservation =
  | { kind: "changed" }
  | { kind: "failed"; runtime: DevelopmentRuntime }
  | { kind: "error"; message: string }
  | { kind: "timeout" }
  | { kind: "cancelled" };

/** One explicit build request owns this observation, not the server job itself. */
export async function watchDevelopmentBuild(token: string, previousVersion: string, signal: AbortSignal): Promise<DevelopmentBuildObservation> {
  const deadline = Date.now() + 20 * 60_000;
  while (!signal.aborted && Date.now() < deadline) {
    await pause(signal);
    if (signal.aborted) break;
    const request = new AbortController();
    const cancel = () => request.abort();
    signal.addEventListener("abort", cancel, { once: true });
    const timeout = window.setTimeout(cancel, Math.min(8_000, Math.max(0, deadline - Date.now())));
    try {
      const [health, runtime] = await Promise.all([
        fetchHealth(request.signal), fetchDevelopmentRuntime(token, request.signal),
      ]);
      // Even an adapter that completes after cancellation cannot authorize navigation.
      if (signal.aborted) break;
      if (request.signal.aborted) continue;
      if (health.version !== previousVersion) return { kind: "changed" };
      if (runtime.state === "failed") return { kind: "failed", runtime };
    } catch (error) {
      if (signal.aborted) break;
      if (!request.signal.aborted && error instanceof RuntimeRequestError && ![502, 503, 504].includes(error.status)) {
        return { kind: "error", message: error.message };
      }
      // A transient restart or bounded request timeout is not a failed build.
    } finally {
      window.clearTimeout(timeout);
      signal.removeEventListener("abort", cancel);
      request.abort();
    }
  }
  return { kind: signal.aborted ? "cancelled" : "timeout" };
}

function pause(signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => { window.clearTimeout(timer); signal.removeEventListener("abort", finish); resolve(); };
    const timer = window.setTimeout(finish, 2_000);
    signal.addEventListener("abort", finish, { once: true });
    if (signal.aborted) finish();
  });
}
