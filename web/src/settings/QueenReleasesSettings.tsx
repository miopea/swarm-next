import { useCallback, useState } from "react";
import { fetchQueenReleases, setQueenReleases } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

/**
 * Whether Queen's review is enough to release code (ADR 0131).
 *
 * Off unless the operator turns it on. On, a worker merges and deploys its
 * task when Queen tells it to, and Queen stops asking the operator to approve
 * that. The operator, asking for this: "I want a queen's review to be enough
 * to releas the code".
 */
export default function QueenReleasesSettings({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState<boolean>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setEnabled((await fetchQueenReleases(operatorToken, signal)).enabled);
    } catch {
      if (!signal.aborted) setError("Could not load the release setting. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(next: boolean) {
    setSaving(true);
    setError(undefined);
    try {
      setEnabled((await setQueenReleases(operatorToken, next)).enabled);
    } catch {
      setError("The release setting was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <section id="settings-queen-releases" className="settings-card queen-releases-settings" aria-labelledby="queen-releases-heading">
      <div><p className="eyebrow">Releases</p><h3 id="queen-releases-heading">Let Queen's review release code</h3></div>
      <label className="worker-autostart">
        <input type="checkbox" checked={enabled ?? false} disabled={enabled === undefined || saving} onChange={(event) => void choose(event.target.checked)} />
        <span><strong>Queen releases reviewed work</strong><small>When Queen has reviewed a task, she tells its worker to merge and release it, and the worker does it without asking you.</small></span>
      </label>
      {enabled === true && (
        <small className="queen-releases-cost" role="status">Queen's word is enough for a worker to merge and deploy its own task once the checks pass. You are still asked about production data, credentials, permissions, admin merges and failed checks.</small>
      )}
      {enabled === false && (
        <small className="queen-releases-detail">Off: a merge or deploy that Queen passes on is stopped by Claude's auto mode, and you are asked to approve it.</small>
      )}
      <small className="queen-releases-detail">A change reaches each worker the next time it starts, and Queen at her next review.</small>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
