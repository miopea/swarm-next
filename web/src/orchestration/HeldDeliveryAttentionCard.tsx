import type { HeldDelivery } from "../api";

type Props = {
  held: HeldDelivery[];
  onOpenWorker?: (workerName: string) => void;
  /**
   * Whether that worker has a live session. Decides the VERB, because the two
   * cases are different actions and a button that says Open cannot wake.
   * Absent means unknown, and unknown reads as awake — the old wording.
   */
  workerIsAwake?: (workerName: string) => boolean;
};

/**
 * Work the coordinator is holding because a terminal has an unanswered prompt.
 *
 * Refusing to type into a session with an open question is correct — a briefing
 * delivered into a prompt is worse than one that waited. Doing it silently and
 * forever is not: on 2026-08-22 a Queen review was held 1503 times over twelve
 * hours behind one unanswered prompt, nothing reached Needs you, and the
 * operator concluded the coordination design was wrong.
 *
 * This is the same failure `moving-from-legacy.md` says Swarm removed —
 * "stranded input was a real category, something waiting in a terminal nobody
 * was looking at" — so it is surfaced where the operator already looks.
 */
export default function HeldDeliveryAttentionCard({ held, onOpenWorker, workerIsAwake }: Props) {
  if (held.length === 0) return null;
  const queen = held.find((entry) => entry.subject === "queen-review");
  const oldest = held.reduce((worst, entry) =>
    entry.first_observed_at < worst.first_observed_at ? entry : worst,
  );
  // An unconfirmed wake is not a delivery waiting its turn. The work was never
  // started, nothing will retry it, and the task reads as routed — so it needs
  // its own sentence and its own instruction.
  const unstarted = held.filter((entry) => entry.kind === "wake_uncertain");
  // Not a question anybody has to answer. The prompt holds text that was typed
  // and never sent, and Swarm will not append to it because a later Enter would
  // submit two unrelated instructions as one. Telling the operator to answer a
  // prompt sends them looking for something that is not there.
  const unsent = oldest.kind === "delivery_held_unsent_text";
  const queenUnsent = queen?.kind === "delivery_held_unsent_text";

  return (
    <section className="queen-attention-card held-delivery-card" aria-labelledby="held-delivery-heading">
      <div>
        <p className="eyebrow">Waiting on a terminal</p>
        <h3 id="held-delivery-heading">
          {queen
            ? queenUnsent
              ? "Queen cannot review until her prompt is cleared"
              : "Queen cannot review until a prompt is answered"
            : unstarted.length > 0 && unstarted.length === held.length
              ? held.length === 1
                ? `${oldest.worker_name ?? "A worker"} was assigned work that never started`
                : `${held.length} tasks are assigned to workers that never started them`
              : held.length === 1
              ? unsent
                ? `${oldest.worker_name ?? "A worker"} has an unsent line at its prompt`
                : `${oldest.worker_name ?? "A worker"} has work waiting behind a prompt`
              : `${held.length} things are waiting at worker prompts`}
        </h3>
        <p>
          {queen
            ? queenUnsent
              ? "Her prompt holds unsent text, and Swarm will not add to it. Clear the line and the review resumes on its own."
              : "Nothing gets routed while Queen's terminal has an open question. Answer it and the review resumes on its own."
            : unsent
              ? "This prompt holds unsent text, and Swarm will not add to it. Clear the line and this delivers itself."
              : oldest.kind === "wake_uncertain"
              ? "Swarm could not confirm this worker woke and will not try again, because waking it twice briefs it twice. Wake it yourself and it picks up from there."
              : "Swarm will not type into a terminal with an open question. Answer the prompt and it delivers itself."}
        </p>
        <p className="held-delivery-since">
          Since {new Date(oldest.first_observed_at * 1000).toLocaleString()} · retried {oldest.observations} times
        </p>
      </div>
      {onOpenWorker && (queen ? "Queen" : oldest.worker_name) ? (() => {
        const target = queen ? "Queen" : (oldest.worker_name ?? "");
        // SAY WHICH ACT THIS IS. The card can already be telling the operator to
        // "wake it yourself"; a button labelled Open beside that sentence reads
        // as a different, weaker thing, and for a sleeping worker there is
        // nothing to open. The verb is the promise.
        const awake = workerIsAwake ? workerIsAwake(target) : true;
        return (
          <button type="button" onClick={() => onOpenWorker(target)}>
            {awake ? "Open" : "Wake"} {target}
          </button>
        );
      })() : null}
    </section>
  );
}
