import type { HeldDelivery } from "../api";

export default function DeliveryWaitList({ held }: { held: HeldDelivery[] }) {
  if (held.length === 0) return null;
  const groups = new Map<string, HeldDelivery[]>();
  for (const entry of held) {
    const owner = entry.subject === "queen-review" || entry.subject.startsWith("queen-run:")
      ? "Queen" : entry.worker_name ?? "Unknown worker";
    const entries = groups.get(owner);
    if (entries) entries.push(entry);
    else groups.set(owner, [entry]);
  }
  return <article className="queue-group" aria-label="Held deliveries">
    <header>
      <h2>Delivery waits <span className="queue-count">{held.length}</span></h2>
      <p className="queue-meaning">Last recorded delivery holds, not proof a worker has stopped. Requests for your decision appear in Needs you.</p>
    </header>
    {[...groups].map(([owner, entries]) => <section key={owner}>
      <h3>{owner}</h3>
      <ul>{entries.map((entry) => <li key={`${entry.kind}:${entry.subject}`}>
        <span className="queue-task-title">{entry.kind === "task_message_reconciliation" ? "Queen: reconcile message delivery" : entry.kind === "delivery_held_unsent_text" ? "Last observed hold: unsent text" : "Last observed hold: prompt not ready"}</span>
        <p className="queue-task-meta">{entry.reason}</p>
        <details><summary>Recorded details</summary>
          <p>First observed {new Date(entry.first_observed_at * 1000).toLocaleString()} · {entry.observations} observations</p>
          <p>{entry.last_observed_at == null ? "Last observation time unavailable." : `Last observed ${new Date(entry.last_observed_at * 1000).toLocaleString()}.`} No resolution has been confirmed.</p>
        </details>
      </li>)}</ul>
    </section>)}
  </article>;
}
