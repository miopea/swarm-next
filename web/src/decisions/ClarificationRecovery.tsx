import { useRef, useState } from "react";
import type { ClarificationReconciliation, DecisionClarification } from "../api";

export type ReconcileClarification = (request: ClarificationReconciliation) => Promise<DecisionClarification>;

/** Mounted with the exact claim as key; consent never carries to a newer attempt. */
export default function ClarificationRecovery({ round, onReconcile, onReload }: {
  round: DecisionClarification; onReconcile: ReconcileClarification; onReload: () => void;
}) {
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string>();
  const inFlight = useRef(false);
  async function reconcile(choice: ClarificationReconciliation["choice"]) {
    if (inFlight.current || done || !round.delivery_claim_id || !round.delivery_session_id
      || (choice === "retry" && !acknowledged)) return;
    inFlight.current = true;
    setBusy(true);
    setError(undefined);
    try {
      await onReconcile({ clarification_id: round.id, decision_id: round.decision_id,
        claim_id: round.delivery_claim_id, session_id: round.delivery_session_id,
        choice, acknowledged_duplicate_risk: acknowledged });
      setDone(true);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Recovery could not be recorded. Check the current delivery status before trying again.");
    } finally { inFlight.current = false; setBusy(false); }
  }
  if (done) return <p role="status">Recovery recorded. <button type="button" onClick={onReload}>Refresh delivery status</button></p>;
  return <div role="group" aria-label="Recover question delivery">
    <p>Check the worker first. Swarm cannot tell whether this question reached its terminal.</p>
    <button type="button" disabled={busy} onClick={() => void reconcile("confirm_delivered")}>I checked: the question is there</button>
    <label><input type="checkbox" checked={acknowledged} disabled={busy}
      onChange={event => setAcknowledged(event.target.checked)} /> I checked the worker and accept that retrying could send this question twice.</label>
    <button type="button" disabled={busy || !acknowledged} onClick={() => void reconcile("retry")}>{busy ? "Recording…" : "Retry this question"}</button>
    {error && <div role="alert"><p>{error}</p><button type="button" disabled={busy} onClick={onReload}>Refresh delivery status</button></div>}
    <p className="muted">This only handles question delivery. It does not approve the task.</p>
  </div>;
}
