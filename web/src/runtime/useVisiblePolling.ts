import { useCallback, useEffect, useRef } from "react";

/** One visible-page request at a time, with a bounded lifetime and explicit owner. */
export function useVisiblePolling(
  task: (signal: AbortSignal) => Promise<void>,
  enabled: boolean,
  intervalMs: number,
  timeoutMs = 8_000,
) {
  const refreshRef = useRef<() => Promise<void>>(async () => undefined);
  useEffect(() => {
    let disposed = false;
    let controller: AbortController | undefined;
    let pending: Promise<void> | undefined;
    let deadline: number | undefined;
    let resumeRequested = false;
    const refresh = (): Promise<void> => {
      if (disposed || !enabled || document.visibilityState === "hidden") return Promise.resolve();
      if (pending) return pending;
      const request = new AbortController();
      controller = request;
      deadline = window.setTimeout(() => request.abort(new DOMException("Polling request timed out", "TimeoutError")), timeoutMs);
      pending = Promise.resolve().then(() => {
        if (!request.signal.aborted) return task(request.signal);
      }).catch(() => {
        // The task owns presentation of failures. Polling must remain recoverable.
      }).finally(() => {
        window.clearTimeout(deadline);
        deadline = undefined;
        if (controller === request) {
          controller = undefined;
          pending = undefined;
          if (resumeRequested) {
            resumeRequested = false;
            void refresh();
          }
        }
      });
      return pending;
    };
    refreshRef.current = refresh;
    const visibility = () => {
      if (document.visibilityState === "hidden") controller?.abort();
      else if (pending && controller?.signal.aborted) resumeRequested = true;
      else void refresh();
    };
    if (enabled) {
      void refresh();
      document.addEventListener("visibilitychange", visibility);
    }
    const timer = enabled ? window.setInterval(() => void refresh(), intervalMs) : undefined;
    return () => {
      disposed = true;
      controller?.abort();
      window.clearTimeout(deadline);
      if (timer !== undefined) window.clearInterval(timer);
      document.removeEventListener("visibilitychange", visibility);
      refreshRef.current = async () => undefined;
    };
  }, [task, enabled, intervalMs, timeoutMs]);
  return useCallback(() => refreshRef.current(), []);
}
