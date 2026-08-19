import { useCallback, useEffect, useState } from "react";

import { fetchDevelopmentRuntime, fetchHealth, fetchTerminalHostStatus } from "../api";
import { nextRuntimeUpdate, type RuntimeUpdateSummary } from "./runtimeUpdates";

const RUNTIME_UPDATE_REFRESH_MS = 15_000;

/**
 * What the control room should say about runtime updates, kept current on its own.
 *
 * This used to be set only as a side effect of refreshing the control room,
 * which nothing does on load or on a timer — so the header indicator appeared
 * only if the operator happened to press refresh or sync Jira, and was
 * otherwise absent while an update sat waiting. The settings page knew,
 * because it polls; the header did not, because it did not.
 *
 * A refresh that learns nothing keeps the previous answer rather than reporting
 * silence as "nothing to update", which is what an App and API build restarting
 * the API looks like from here.
 */
export function useRuntimeUpdate(
  operatorToken: string | undefined,
  refreshMs = RUNTIME_UPDATE_REFRESH_MS,
) {
  const [update, setUpdate] = useState<RuntimeUpdateSummary>();

  const refresh = useCallback(async () => {
    if (!operatorToken) return;
    const [health, host, development] = await Promise.all([
      fetchHealth().catch(() => undefined),
      fetchTerminalHostStatus(operatorToken).catch(() => undefined),
      fetchDevelopmentRuntime(operatorToken).catch(() => undefined),
    ]);
    setUpdate((previous) => nextRuntimeUpdate(previous, health, host, development));
  }, [operatorToken]);

  useEffect(() => {
    if (!operatorToken) {
      setUpdate(undefined);
      return;
    }
    void refresh();
    const interval = window.setInterval(() => void refresh(), refreshMs);
    return () => window.clearInterval(interval);
  }, [operatorToken, refresh, refreshMs]);

  return { runtimeUpdate: update, refreshRuntimeUpdate: refresh };
}
