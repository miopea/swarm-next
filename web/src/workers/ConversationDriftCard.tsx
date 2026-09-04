/**
 * Workers whose next start would resume a conversation Swarm cannot vouch for.
 *
 * Swarm pins a conversation id when a worker is created and never learns when
 * the real one moves. Resuming a different thread inside the session — exactly
 * what an operator does to recover work — is invisible to Swarm, so the next
 * start drops the worker back into the older conversation and silently
 * regresses its state.
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
  // ⚠️ "UNKNOWN" IS NOT ON THIS PAGE, AND THE OPERATOR IS WHY.
  //
  // They were shown eight rows under "What needs you" and said "there is nothing
  // I can do about it". Five were unknown — Wifi Portal, ShotCraft, Aria,
  // Operations Report, Silly Tavern — and every one has ZERO transcripts in its
  // workspace, because those workers have never run there. "Swarm could not tell
  // which conversation is newest" is the ORDINARY state of a worker that has
  // never started, not a fault, and there is no second thread to choose between.
  //
  // The card's own closing line only makes sense for stale: which thread is the
  // right one is a judgement about your work. An unknown worker has no thread to
  // judge.
  //
  // THIS DOES NOT ASSUME THEM HEALTHY. The server still reports Unknown and
  // still refuses to call it current, and worker start handles a pin Claude has
  // never held by starting fresh under the same id. What changed is that it
  // stopped being filed as the operator's move.
  //
  // One Unknown reason IS a real fault — "the Claude project directory could not
  // be read" is a permissions problem rather than a never-run worker. It needs
  // its own signal rather than riding along with five benign rows, and is
  // deliberately not smuggled back by matching on reason text.
  if (stale.length === 0) return null;

  return (
    <article className="attention-card conversation-drift" aria-label="Worker conversations">
      <header>
        <strong>
          {stale.length > 0
            ? `${stale.length} worker${stale.length === 1 ? "" : "s"} would resume an older conversation`
            : "Some worker conversations could not be checked"}
        </strong>
      </header>
      {stale.length > 0 ? (
        <>
          <p>
            Starting {stale.length === 1 ? "this worker" : "these workers"} resumes a thread that is
            not the newest one in its workspace, which loses whatever happened since.
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
