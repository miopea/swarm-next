import { useCallback, useEffect, useState } from "react";

import { fetchDevelopmentRuntime, fetchHealth, fetchProviderCapabilities, fetchTerminalHostStatus } from "../api";
import { nextRuntimeUpdates, type RuntimeUpdateSummary } from "./runtimeUpdates";

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
  const [updates, setUpdates] = useState<RuntimeUpdateSummary[]>();

  const refresh = useCallback(async () => {
    if (!operatorToken) return;
    const [health, host, development, providers] = await Promise.all([
      fetchHealth().catch(() => undefined),
      fetchTerminalHostStatus(operatorToken).catch(() => undefined),
      fetchDevelopmentRuntime(operatorToken).catch(() => undefined),
      fetchProviderCapabilities(operatorToken).catch(() => undefined),
    ]);
    setUpdates((previous) =>
      nextRuntimeUpdates(previous, health, host, development, providers?.superseded ?? []));
  }, [operatorToken]);

  useEffect(() => {
    if (!operatorToken) {
      setUpdates(undefined);
      return;
    }
    void refresh();
    const interval = window.setInterval(() => void refresh(), refreshMs);
    return () => window.clearInterval(interval);
  }, [operatorToken, refresh, refreshMs]);

  return { runtimeUpdates: updates ?? [], refreshRuntimeUpdate: refresh };
}
