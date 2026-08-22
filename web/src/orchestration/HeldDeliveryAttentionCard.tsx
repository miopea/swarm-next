import type { HeldDelivery } from "../api";

type Props = {
  held: HeldDelivery[];
  onOpenWorker?: (workerName: string) => void;
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
export default function HeldDeliveryAttentionCard({ held, onOpenWorker }: Props) {
  if (held.length === 0) return null;
  const queen = held.find((entry) => entry.subject === "queen-review");
  const oldest = held.reduce((worst, entry) =>
    entry.first_observed_at < worst.first_observed_at ? entry : worst,
  );

  return (
    <section className="queen-attention-card held-delivery-card" aria-labelledby="held-delivery-heading">
      <div>
        <p className="eyebrow">Waiting on a terminal</p>
        <h3 id="held-delivery-heading">
          {queen
            ? "Queen cannot review until a prompt is answered"
            : held.length === 1
              ? `${oldest.worker_name ?? "A worker"} has work waiting behind a prompt`
              : `${held.length} things are waiting behind unanswered prompts`}
        </h3>
        <p>
          {queen
            ? "Nothing reaches this queue and nothing gets routed while Queen's terminal has an open question. Answer it and the review resumes on its own."
            : "Swarm will not type into a terminal with an open question, so this is waiting rather than lost. Answer the prompt and it delivers itself."}
        </p>
        <p className="held-delivery-since">
          Since {new Date(oldest.first_observed_at * 1000).toLocaleString()} · retried {oldest.observations} times
        </p>
      </div>
      {onOpenWorker && (queen ? "Queen" : oldest.worker_name) ? (
        <button type="button" onClick={() => onOpenWorker(queen ? "Queen" : (oldest.worker_name ?? ""))}>
          Open {queen ? "Queen" : oldest.worker_name}
        </button>
      ) : null}
    </section>
  );
}
