import { useEffect, useState } from "react";

import { fetchDevelopmentRuntime, type DevelopmentRuntime } from "../api";

const DEVELOPMENT_STATUS_REFRESH_MS = 15_000;

/**
 * The development runtime, and whether the API is currently answering about it.
 *
 * The last known runtime is kept when a refresh fails. Activating a build
 * restarts the API, so the one moment this call reliably fails is the middle of
 * the operation the operator is watching — and discarding what we knew there
 * made the App and API card vanish exactly then.
 */
export function useDevelopmentRuntime(
  operatorToken: string,
  runningVersion: string | undefined,
  refreshMs = DEVELOPMENT_STATUS_REFRESH_MS,
): { runtime: DevelopmentRuntime | undefined; reachable: boolean } {
  const [runtime, setRuntime] = useState<DevelopmentRuntime>();
  const [reachable, setReachable] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const next = await fetchDevelopmentRuntime(operatorToken);
        if (cancelled) return;
        setRuntime(next);
        setReachable(true);
      } catch {
        if (!cancelled) setReachable(false);
      }
    };

    void refresh();
    const interval = window.setInterval(() => void refresh(), refreshMs);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [operatorToken, refreshMs, runningVersion]);

  return { runtime, reachable };
}
