import type { HeldDelivery } from "../api";

export default function WorkerStartNotice({ held, unavailable, onDiagnostics }: {
  held: HeldDelivery[];
  unavailable: boolean;
  onDiagnostics: () => void;
}) {
  if (held.length === 0) return null;
  const reasons = [...new Set(held.map((entry) => entry.reason || "The coordinator has not provided a reason."))];
  return <details className="runtime-update-card runtime-start-notice" aria-label="Worker start safeguards">
    <summary>Worker starts paused · {held.length} {held.length === 1 ? "item" : "items"}</summary>
    <p className="runtime-update-detail">Swarm is holding new starts until its resource safety checks permit them. This is not a request for approval.</p>
    {unavailable && <p className="runtime-update-detail" role="status">Status could not be refreshed. These are the last recorded holds, not a confirmed current check.</p>}
    {reasons.map((reason) => <p key={reason} className="runtime-update-detail">{reason}</p>)}
    <button type="button" className="runtime-update-run" onClick={onDiagnostics}>Check diagnostics</button>
  </details>;
}
