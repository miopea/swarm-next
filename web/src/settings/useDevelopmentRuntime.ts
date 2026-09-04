import { useCallback, useState } from "react";

import { fetchDevelopmentRuntime, type DevelopmentRuntime } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

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

  const refresh = useCallback(async (signal: AbortSignal) => {
      try {
        const next = await fetchDevelopmentRuntime(operatorToken, signal);
        if (signal.aborted) return;
        setRuntime(next);
        setReachable(true);
      } catch {
        if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) setReachable(false);
      }
  }, [operatorToken, runningVersion]);
  useVisiblePolling(refresh, Boolean(operatorToken), refreshMs);

  return { runtime, reachable };
}
