import { useCallback, useEffect, useState } from "react";

import { fetchDevelopmentRuntime, fetchHealth, fetchProviderCapabilities, fetchTerminalHostStatus } from "../api";
import { nextRuntimeUpdates, type RuntimeUpdateSummary } from "./runtimeUpdates";
import { useVisiblePolling } from "./useVisiblePolling";

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
  // Whether this Hive builds from a working copy. The operator asked to be able
  // to see that without opening Settings, because it changes what every other
  // line here means.
  const [developmentMode, setDevelopmentMode] = useState(false);

  const poll = useCallback(async (signal: AbortSignal) => {
    if (!operatorToken) return;
    const [health, host, development, providers] = await Promise.all([
      fetchHealth(signal).catch(() => undefined),
      fetchTerminalHostStatus(operatorToken, signal).catch(() => undefined),
      fetchDevelopmentRuntime(operatorToken, signal).catch(() => undefined),
      fetchProviderCapabilities(operatorToken, signal).catch(() => undefined),
    ]);
    if (signal.aborted) return;
    // An API restart is unknown status, not a switch to a release installation.
    if (development) setDevelopmentMode(development.enabled);
    setUpdates((previous) =>
      nextRuntimeUpdates(previous, health, host, development, providers?.superseded ?? []));
  }, [operatorToken]);

  const refresh = useVisiblePolling(poll, Boolean(operatorToken), refreshMs);

  useEffect(() => {
    if (!operatorToken) {
      setUpdates(undefined);
      setDevelopmentMode(false);
      return;
    }
  }, [operatorToken]);

  return { runtimeUpdates: updates ?? [], developmentMode, refreshRuntimeUpdate: refresh };
}
