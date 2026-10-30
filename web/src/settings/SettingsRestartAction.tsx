import { useCallback, useState } from "react";

import { fetchSettingsRestart, type WorkerBehindSettings } from "../api";
import { settingNames, settingsBehindNames } from "../runtime/runtimeUpdates";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

function runningWorkers(count: number): string {
  return `${count} running worker${count === 1 ? "" : "s"}`;
}

/**
 * Running workers still on an older value of a setting they read only when they
 * start (ADR 0132): the worker model, Remote Control, Queen releases, and each
 * worker's system access.
 *
 * Each of those cards says a change reaches a worker the next time it starts.
 * This is the card that says which workers that still leaves behind, so the
 * operator is not the one keeping count.
 */
export default function SettingsRestartAction({ operatorToken, busy, onRestart }: {
  operatorToken: string;
  busy: boolean;
  onRestart: () => Promise<void>;
}) {
  const [behind, setBehind] = useState<WorkerBehindSettings[]>();
  const [unavailable, setUnavailable] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      const status = await fetchSettingsRestart(operatorToken, signal);
      if (!signal.aborted) { setBehind(status.workers); setUnavailable(false); }
    } catch {
      if (!signal.aborted) setUnavailable(true);
    }
  }, [operatorToken]);
  const refresh = useVisiblePolling(load, Boolean(operatorToken), 15_000);

  if (unavailable && !behind) {
    return (
      <article className="runtime-subsystem-card settings-restart-action" aria-label="Settings waiting for a restart">
        <header>
          <div><span className="runtime-component-name">Worker settings</span><strong>Could not check</strong></div>
          <span className="runtime-status-badge unavailable">Unavailable</span>
        </header>
        <p>Swarm could not read which workers are still running with an older setting.</p>
      </article>
    );
  }
  if (!behind || behind.length === 0) return null;
  const count = behind.length;
  return (
    <article className="runtime-subsystem-card runtime-subsystem-restart settings-restart-action" aria-label="Settings waiting for a restart">
      <header>
        <div><span className="runtime-component-name">Worker settings</span><strong>{settingsBehindNames(behind)} changed · restart to apply</strong></div>
        <span className="runtime-status-badge restart">Restart required</span>
      </header>
      <p>{count === 1 ? "This worker" : `These ${count} workers`} started before the change and {count === 1 ? "keeps" : "keep"} the old setting until {count === 1 ? "it restarts" : "they restart"}:</p>
      <ul className="settings-restart-workers">
        {behind.map((worker) => (
          <li key={worker.worker_id}><strong>{worker.name}</strong>: {settingNames(worker.settings)}</li>
        ))}
      </ul>
      {unavailable ? <small role="status">This list could not be refreshed and may be out of date.</small> : null}
      {!confirming ? (
        <button className="secondary-button" type="button" disabled={busy || unavailable} onClick={() => setConfirming(true)}>Restart {runningWorkers(count)}</button>
      ) : (
        <div className="maintenance-confirmation" role="group" aria-label="Confirm restarting workers for settings">
          <strong>Restart {runningWorkers(count)} now?</strong>
          <span>Anything they are running is interrupted. Swarm attempts to resume their saved conversations with the new settings. Workers already on the current settings are left alone.</span>
          <div className="settings-actions">
            <button className="secondary-button" type="button" disabled={busy} onClick={() => setConfirming(false)}>Not now</button>
            <button className="primary-action" type="button" disabled={busy} onClick={() => { setConfirming(false); void onRestart().then(refresh); }}>Restart and apply</button>
          </div>
        </div>
      )}
    </article>
  );
}
