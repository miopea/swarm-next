import { useCallback, useEffect, useMemo, useState } from "react";
import { recordBrowserEvidence } from "../api";
import { browserPerformance } from "./browserPerformance";
import { HourlyBrowserEvidence } from "./hourlyBrowserEvidence";
import { useVisiblePolling } from "./useVisiblePolling";

export type DogfoodCollectionStatus = {
  state: "disabled" | "collecting" | "unavailable";
  dropped_samples: number;
  pruned_captures: number;
};

/** App-owned, single-flight uploads; Settings does not own collection lifetime. */
export function useDogfoodCollection(token: string | undefined, enabled: boolean, build: string | undefined) {
  const collector = useMemo(() => {
    if (!token || !enabled || !build || !/^[a-zA-Z0-9.+_-]{1,128}$/.test(build)) return undefined;
    return new HourlyBrowserEvidence(build);
  }, [token, enabled, build]);
  const [status, setStatus] = useState<DogfoodCollectionStatus>({ state: "disabled", dropped_samples: 0, pruned_captures: 0 });
  useEffect(() => {
    setStatus({ state: collector ? "collecting" : "disabled", dropped_samples: 0, pruned_captures: 0 });
    if (!collector) return;
    return browserPerformance.attachHourlySink((metric, duration, at) => {
      if (document.visibilityState === "visible") collector.record(metric, duration, at);
    });
  }, [collector]);

  const flush = useCallback(async (signal: AbortSignal) => {
    if (!collector || !token) return;
    try {
      // One request per minute/visible return. Never fan out or recursively retry.
      const capture = collector.next(Date.now());
      if (!capture) {
        setStatus((previous) => previous.dropped_samples === collector.status.dropped_samples ? previous
          : { ...previous, dropped_samples: collector.status.dropped_samples });
        return;
      }
      const result = await recordBrowserEvidence(token, capture, signal);
      if (signal.aborted) return;
      collector.acknowledge(capture);
      setStatus((previous) => ({ state: "collecting", dropped_samples: collector.status.dropped_samples,
        pruned_captures: previous.pruned_captures + result.pruned }));
    } catch {
      if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) {
        setStatus((previous) => ({ ...previous, state: "unavailable", dropped_samples: collector.status.dropped_samples }));
      }
    }
  }, [collector, token]);
  useVisiblePolling(flush, Boolean(collector), 60_000);
  return status;
}
