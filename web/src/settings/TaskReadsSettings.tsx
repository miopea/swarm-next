import { useCallback, useState } from "react";
import { fetchTaskReads, setTaskReads } from "../api";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

/**
 * Whether a worker reads production for its own task without asking (ADR 0146).
 *
 * Off unless the operator turns it on. On, a worker runs a read-only check for
 * the task it holds rather than filing a command request for it. Writes,
 * spend and personal data still come to the operator. The operator, choosing
 * this: "Read-only runs unasked".
 */
export default function TaskReadsSettings({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState<boolean>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setEnabled((await fetchTaskReads(operatorToken, signal)).enabled);
    } catch {
      if (!signal.aborted) setError("Could not load the reads setting. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(next: boolean) {
    setSaving(true);
    setError(undefined);
    try {
      setEnabled((await setTaskReads(operatorToken, next)).enabled);
    } catch {
      setError("The reads setting was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <section id="settings-task-reads" className="settings-card task-reads-settings" aria-labelledby="task-reads-heading">
      <div><p className="eyebrow">Production reads</p><h3 id="task-reads-heading">Let workers read production for their task</h3></div>
      <label className="worker-autostart">
        <input type="checkbox" checked={enabled ?? false} disabled={enabled === undefined || saving} onChange={(event) => void choose(event.target.checked)} />
        <span><strong>Workers read production for their task</strong><small>A worker runs a read-only check for the task it holds, such as a page fetch, a read-only query, a log search or a cloud list, without asking you first.</small></span>
      </label>
      {enabled === true && (
        <small className="task-reads-cost" role="status">Workers read counts and aggregates for their own task without a card. You are still asked about every write, setting or permission change, anything that spends money, and any read of names, addresses or payment details. Deploys are not reads: the Queen releases setting decides those.</small>
      )}
      {enabled === false && (
        <small className="task-reads-detail">Off: Claude's auto mode stops a worker's production read, and you are asked to allow it.</small>
      )}
      <small className="task-reads-detail">A change reaches each worker the next time it starts. Until then, the left panel lists the workers still on the old setting and can restart them.</small>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
