import { useCallback, useState } from "react";
import { fetchWorkerRemoteControl, setWorkerRemoteControl } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

/**
 * Whether Claude workers connect to Remote Control (ADR 0122).
 *
 * Off unless the operator turns it on, and the card says what turning it on
 * costs: Claude refuses /restart while Remote Control is connected, so Swarm
 * cannot move a connected worker onto a newly installed Claude. The operator,
 * asking for this: "set this in settings to turn on /rc or not and what it
 * means if you do."
 */
export default function WorkerRemoteControlSettings({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState<boolean>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setEnabled((await fetchWorkerRemoteControl(operatorToken, signal)).enabled);
    } catch {
      if (!signal.aborted) setError("Could not load the Remote Control setting. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(next: boolean) {
    setSaving(true);
    setError(undefined);
    try {
      setEnabled((await setWorkerRemoteControl(operatorToken, next)).enabled);
    } catch {
      setError("The Remote Control setting was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <section id="settings-worker-remote-control" className="settings-card worker-remote-control-settings" aria-labelledby="worker-remote-control-heading">
      <div><p className="eyebrow">Remote Control</p><h3 id="worker-remote-control-heading">Follow workers in the Claude app</h3></div>
      <label className="worker-autostart">
        <input type="checkbox" checked={enabled ?? false} disabled={enabled === undefined || saving} onChange={(event) => void choose(event.target.checked)} />
        <span><strong>Let Remote Control connect to workers</strong><small>Each Claude worker also appears in the Claude app and at claude.ai/code, where you can read it and type to it.</small></span>
      </label>
      {enabled === true && (
        <small className="worker-remote-control-cost" role="status">While a worker is connected, Claude will not restart it, so Swarm cannot move it onto a new Claude version. It stays on the older one until it is stopped and started again.</small>
      )}
      {enabled === false && (
        <small className="worker-remote-control-detail">Off: you follow workers here in Swarm, and Swarm moves each one onto a new Claude version by itself once it is free.</small>
      )}
      <small className="worker-remote-control-detail">A change reaches each worker the next time it starts. Until then, the left panel lists the workers still on the old setting and can restart them. Codex workers are not affected.</small>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
