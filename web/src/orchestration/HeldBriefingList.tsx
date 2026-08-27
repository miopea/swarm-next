import type { HeldBriefing } from "../api";

type Props = {
  briefings: HeldBriefing[];
  onOpenTask?: (taskId: string) => void;
};

/**
 * Briefings queued and not yet handed over.
 *
 * DELIBERATELY NOT AN ATTENTION CARD, and the distinction matters. A held
 * DELIVERY was attempted and refused — one was retried 1503 times over twelve
 * hours behind a single unanswered prompt, and nothing was moving. A held
 * BRIEFING was never attempted, because the dispatcher correctly declined to
 * claim it: the worker is mid-task, or an earlier task is ahead of it, or a
 * person is typing in that terminal. Those are a queue working.
 *
 * They also clear themselves. Two on this Hive went to zero within minutes,
 * with nothing done about them, which is why this does not touch the Needs-you
 * count: badging a state that resolves on its own is how an operator learns to
 * ignore the badge, and the badge only works while they believe it.
 *
 * It is rendered rather than dropped because the server computed it and no
 * surface read it. The value is in the AGE — "waiting its turn" is benign for
 * minutes and is a stalled predecessor after hours, and only the operator can
 * tell which by looking.
 */
export default function HeldBriefingList({ briefings, onOpenTask }: Props) {
  if (briefings.length === 0) return null;
  const now = Date.now() / 1000;
  return (
    <section className="held-briefing-list" aria-labelledby="held-briefing-heading">
      <p className="eyebrow">Briefings waiting their turn</p>
      <h4 id="held-briefing-heading">
        {briefings.length === 1 ? "One briefing is queued" : `${briefings.length} briefings are queued`}
      </h4>
      <p className="held-briefing-note">
        Nothing is wrong with these — Swarm is holding them until the worker is free. Worth a look
        only if one has been waiting far longer than the task ahead of it should take.
      </p>
      <ul>
        {briefings.map((briefing) => (
          <li key={briefing.task_id}>
            <button type="button" className="link-button" onClick={() => onOpenTask?.(briefing.task_id)}>
              {briefing.title}
            </button>
            <span className="held-briefing-detail">
              {briefing.worker_name} · {holdReason(briefing)} · waiting {waitedFor(now - briefing.queued_at)}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * Why this one has not been handed over, in the operator's terms.
 *
 * "waiting_its_turn" alone is unfalsifiable, which is recorded in the
 * persistence layer after sixteen briefings reported it at once and named
 * nothing to go and look at. When the earlier task is known, it is named.
 */
function holdReason(briefing: HeldBriefing): string {
  switch (briefing.reason) {
    case "operator_in_the_terminal":
      return "you are in that terminal";
    case "worker_already_working":
      return "the worker is on something else";
    case "waiting_its_turn":
      return briefing.blocked_by ? `behind ${briefing.blocked_by}` : "behind earlier work";
    default:
      return briefing.reason;
  }
}

/** Coarse on purpose: the question is hours-or-minutes, not the exact figure. */
function waitedFor(seconds: number): string {
  if (seconds < 90) return "under a minute";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} minutes`;
  const hours = seconds / 3600;
  return hours < 10 ? `${hours.toFixed(1)} hours` : `${Math.round(hours)} hours`;
}
