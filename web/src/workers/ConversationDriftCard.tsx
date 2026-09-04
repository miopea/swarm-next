/**
 * Conversation defaults with positive evidence worth reviewing.
 *
 * Confirmed provider selections take precedence on the server. These remaining
 * rows come from the older transcript-recency check and are reasons to review
 * a default, not proof that a newer transcript is the operator's intended one.
 *
 * Unknown histories remain visible in runtime details. They are not evidence
 * of a wrong default and must not manufacture an operator action here.
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
  if (stale.length === 0) return null;

  return (
    <article className="attention-card conversation-drift" aria-label="Worker conversations">
      <header>
        <strong>
          {`${stale.length} conversation default${stale.length === 1 ? "" : "s"} to review`}
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
      <small>
        Swarm does not switch for you: which thread is the right one is a judgement about your work,
        not about timestamps. Open the worker and resume the one you want.
      </small>
    </article>
  );
}
