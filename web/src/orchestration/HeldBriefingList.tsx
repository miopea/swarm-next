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
        These briefings have not reached their workers. The recorded reason below explains each wait; age alone does not require your approval.
      </p>
      {/* GROUPED BY WORKER, because that is what the repetition actually is.
          The operator's screenshot showed seven rows each ending
          "BFG Watchfaces · the worker is on something else · waiting 41
          minutes", four of them identical but for the title — the same fact
          restated four times, with the eye travelling the width of the window
          to read it each time.

          By worker rather than by reason: the reason is the same sentence for
          every row in that screenshot, so grouping on it makes one group and
          explains nothing. "Four of these are behind one worker" is the fact
          worth seeing, and it is what tells the operator whether to look. */}
      {groupByWorker(briefings).map((group) => (
        <section className="held-briefing-group" key={group.workerId}>
          <p className="held-briefing-group-heading">
            <strong>{group.workerName}</strong>
            {group.sharedReason && <> · {group.sharedReason}</>}
            {" · "}
            {group.briefings.length === 1
              ? `waiting ${waitedFor(now - group.briefings[0].queued_at)}`
              : `${group.briefings.length} briefings, longest waiting ${waitedFor(now - Math.min(...group.briefings.map((briefing) => briefing.queued_at)))}`}
          </p>
          <ul className="held-briefing-rows">
            {group.briefings.map((briefing) => (
              <li key={briefing.task_id}>
                {/* Reset explicitly. A bare <button> in this app is the big
                    filled control, so a title left unstyled renders as a
                    full-width gold call to action — louder than the attention
                    card above it, on a panel whose whole point is that nothing
                    is wrong. */}
                <button
                  type="button"
                  className="held-briefing-title"
                  onClick={() => onOpenTask?.(briefing.task_id)}
                >
                  {briefing.title}
                </button>
                {!group.sharedReason && <p className="queue-task-meta">{holdReason(briefing)}</p>}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </section>
  );
}

/**
 * The briefings behind each worker, in the order the workers first appear.
 *
 * Insertion order rather than sorted, so a list the operator has already looked
 * at does not reshuffle under them when one group gains a briefing.
 */
function groupByWorker(briefings: HeldBriefing[]): { workerId: string; workerName: string; sharedReason: string | null; briefings: HeldBriefing[] }[] {
  const groups = new Map<string, HeldBriefing[]>();
  for (const briefing of briefings) {
    const existing = groups.get(briefing.worker_id);
    if (existing) existing.push(briefing);
    else groups.set(briefing.worker_id, [briefing]);
  }
  return [...groups].map(([workerId, grouped]) => ({
    workerId,
    workerName: grouped[0].worker_name,
    sharedReason: grouped.every((briefing) => holdReason(briefing) === holdReason(grouped[0]))
      ? holdReason(grouped[0]) : null,
    briefings: grouped,
  }));
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
    case "experimental_during_night_watch":
      return "experimental provider — queued until Night Watch ends";
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
