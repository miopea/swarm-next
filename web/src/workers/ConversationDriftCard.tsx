/**
 * Conversation defaults with positive evidence worth reviewing.
 *
 * Confirmed provider selections take precedence on the server. These remaining
 * rows come from the older transcript-recency check and are reasons to review
 * a default, not proof that a newer transcript is the operator's intended one.
 *
 * Benign unknown histories remain visible in runtime details. A server-marked
 * filesystem fault is actionable and stays on Needs You without pretending a
 * worker that never ran has a conversation choice to make.
 *
 * It does NOT offer to switch. Picking the newest thread on the operator's
 * behalf is a guess about which one they wanted, and a wrong guess is the same
 * regression from the other direction — they declined that explicitly.
 */
export type ConversationFreshness =
  | { state: "current" }
  | { state: "stale"; newest_conversation: string; pinned_last_entry: string | null; newest_last_entry: string }
  // `cause` is optional only across rolling compatibility with an older API.
  // Absence never becomes actionable by guess; retire this compatibility once
  // every supported API emits the server-owned verdict.
  | { state: "unknown"; cause?: UnknownCause; reason: string };

/**
 * Why Swarm could not tell, as a value with the verdict already attached.
 *
 * ⚠️ `fault` IS DECIDED BY THE SERVER AND MUST NOT BE RE-DERIVED FROM `kind`.
 * A list of fault kinds here would be a second owner of one fact, and a cause
 * added in worker_runtime.rs and forgotten in this file is exactly the silent
 * wrong answer this whole distinction exists to prevent. Read `fault`; `kind`
 * is for display and for tests.
 */
export type UnknownCause = { kind: string; fault: boolean };

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
  // A DIRECTORY THAT EXISTS AND CANNOT BE READ IS SOMEBODY'S TO FIX, and it is
  // the one Unknown that belongs on this page. Selected on the server's own
  // verdict rather than on the reason sentence — see UnknownCause above.
  const faults = workers.filter(
    (worker) => worker.freshness.state === "unknown" && worker.freshness.cause?.fault === true,
  );
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
  if (stale.length === 0 && faults.length === 0) return null;

  return (
    <article className="attention-card conversation-drift" aria-label="Worker conversations">
      <header>
        <strong>
          {stale.length > 0
            ? `${stale.length} worker${stale.length === 1 ? "" : "s"} would resume an older conversation`
            : `Swarm cannot read the conversation history for ${faults.length} worker${faults.length === 1 ? "" : "s"}`}
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
      {faults.length > 0 ? (
        <>
          <p>
            {faults.length === 1 ? "This worker's" : "These workers'"} conversation history exists
            and could not be read, so Swarm cannot tell whether a start would resume the right
            thread. That is a permissions or filesystem problem on this machine, not a worker that
            has never run.
          </p>
          <ul>
            {faults.map((worker) => {
              const freshness = worker.freshness as Extract<ConversationFreshness, { state: "unknown" }>;
              return (
                <li key={worker.worker_id}>
                  <button type="button" onClick={() => onOpenWorker(worker.worker_id)}>{worker.name}</button>
                  <small>{freshness.reason}.</small>
                </li>
              );
            })}
          </ul>
        </>
      ) : null}
      {stale.length > 0 ? (
        <small>
          Swarm does not switch for you: which thread is the right one is a judgement about your
          work, not about timestamps. Open the worker and resume the one you want.
        </small>
      ) : null}
    </article>
  );
}
