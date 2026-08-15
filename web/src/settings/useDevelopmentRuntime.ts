import { useEffect, useState } from "react";

import { fetchDevelopmentRuntime, type DevelopmentRuntime } from "../api";

const DEVELOPMENT_STATUS_REFRESH_MS = 15_000;

export function useDevelopmentRuntime(
  operatorToken: string,
  runningVersion: string | undefined,
  refreshMs = DEVELOPMENT_STATUS_REFRESH_MS,
) {
  const [runtime, setRuntime] = useState<DevelopmentRuntime>();

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const next = await fetchDevelopmentRuntime(operatorToken);
        if (!cancelled) setRuntime(next);
      } catch {
        if (!cancelled) setRuntime(undefined);
      }
    };

    void refresh();
    const interval = window.setInterval(() => void refresh(), refreshMs);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [operatorToken, refreshMs, runningVersion]);

  return runtime;
}
