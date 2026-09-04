/**
 * Workers whose next start would resume a conversation Swarm cannot vouch for.
 *
 * Confirmed provider selections take precedence on the server. These remaining
 * rows come from the older transcript-recency check and are reasons to review
 * a default, not proof that a newer transcript is the operator's intended one.
 *
 * BOTH CASES ARE SHOWN, and the second is the operator's own requirement: "We
 * need a way to notify if we don't know." An unknown that reads as healthy is
 * the failure this Hive keeps rediscovering, so a worker Swarm cannot check is
 * listed here rather than quietly assumed fine.
 *
 * It does NOT offer to switch. Picking the newest thread on the operator's
 * behalf is a guess about which one they wanted, and a wrong guess is the same
 * regression from the other direction — they declined that explicitly.
 */
export type ConversationFreshness =
  | { state: "current" }
  | { state: "stale"; newest_conversation: string; pinned_last_entry: string | null; newest_last_entry: string }
  | { state: "unknown"; reason: string };

export type WorkerConversation = { worker_id: string; name: string; freshness: ConversationFreshness };

function when(timestamp: string | null): string {
  if (!timestamp) return "never";
  const parsed = Date.parse(timestamp);
  if (Number.isNaN(parsed)) return timestamp;
  return new Date(parsed).toLocaleString();
}

export default function ConversationDriftCard({
  workers,
  onOpenWorker,
}: {
  workers: WorkerConversation[];
  onOpenWorker: (workerId: string) => void;
}) {
  const stale = workers.filter((worker) => worker.freshness.state === "stale");
  const unknown = workers.filter((worker) => worker.freshness.state === "unknown");
  if (stale.length === 0 && unknown.length === 0) return null;

  return (
    <article className="attention-card conversation-drift" aria-label="Worker conversations">
      <header>
        <strong>
          {stale.length > 0
            ? `${stale.length} conversation default${stale.length === 1 ? "" : "s"} to review`
            : "Some worker conversations could not be checked"}
        </strong>
      </header>
      {stale.length > 0 ? (
        <>
          <p>
            Newer conversation history exists. Check that the saved default is still the conversation you want.
          </p>
          <ul>
            {stale.map((worker) => {
              const freshness = worker.freshness as Extract<ConversationFreshness, { state: "stale" }>;
              return (
                <li key={worker.worker_id}>
                  <button type="button" onClick={() => onOpenWorker(worker.worker_id)}>{worker.name}</button>
                  <small>
                    Pinned thread last spoke {when(freshness.pinned_last_entry)}; the newest spoke{" "}
                    {when(freshness.newest_last_entry)}.
                  </small>
                </li>
              );
            })}
          </ul>
        </>
      ) : null}
      {unknown.length > 0 ? (
        <>
          <p className="conversation-drift-unknown">
            Swarm could not tell which conversation is newest for {unknown.length} worker
            {unknown.length === 1 ? "" : "s"}. That is reported rather than assumed healthy.
          </p>
          <ul>
            {unknown.map((worker) => (
              <li key={worker.worker_id}>
                <button type="button" onClick={() => onOpenWorker(worker.worker_id)}>{worker.name}</button>
                <small>{(worker.freshness as Extract<ConversationFreshness, { state: "unknown" }>).reason}.</small>
              </li>
            ))}
          </ul>
        </>
      ) : null}
      <small>
        Swarm does not switch for you: which thread is the right one is a judgement about your work,
        not about timestamps. Open the worker and resume the one you want.
      </small>
    </article>
  );
}
