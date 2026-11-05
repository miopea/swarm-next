import { useCallback, useState } from "react";
import { fetchQueenGrants, setQueenGrants } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

/**
 * Whether Queen may allow a worker's command for work she reviewed (ADR 0158).
 *
 * Off unless the operator turns it on. On, a command card whose worker said
 * none of the operator's reasons applies is Queen's to grant or hand on. The
 * operator, choosing this: "Queen clears most cards", keeping credentials,
 * permissions, spend over $25 and irreversible deletes.
 */
export default function QueenGrantsSettings({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState<boolean>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setEnabled((await fetchQueenGrants(operatorToken, signal)).enabled);
    } catch {
      if (!signal.aborted) setError("Could not load the command card setting. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(next: boolean) {
    setSaving(true);
    setError(undefined);
    try {
      setEnabled((await setQueenGrants(operatorToken, next)).enabled);
    } catch {
      setError("The command card setting was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <section id="settings-queen-grants" className="settings-card queen-grants-settings" aria-labelledby="queen-grants-heading">
      <div><p className="eyebrow">Command cards</p><h3 id="queen-grants-heading">Let Queen allow commands for work she reviewed</h3></div>
      <label className="worker-autostart">
        <input type="checkbox" checked={enabled ?? false} disabled={enabled === undefined || saving} onChange={(event) => void choose(event.target.checked)} />
        <span><strong>Queen grants command cards</strong><small>When a worker asks to run a command and says none of your reasons applies, Queen reads it and may allow it once instead of asking you.</small></span>
      </label>
      {enabled === true && (
        <small className="queen-grants-cost" role="status">Queen allows a command only for work she has reviewed. You still get every card that touches credentials or permissions, may cost more than $25, or deletes something that cannot be restored, and every card Queen hands to you. A card with Queen is listed on the Queues page, not in Needs you, and a worker must say which of your reasons apply before it can ask.</small>
      )}
      {enabled === false && (
        <small className="queen-grants-detail">Off: every command card comes to you.</small>
      )}
      <small className="queen-grants-detail">Queen hears a change at her next review. Workers start saying which of your reasons apply once they restart.</small>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
