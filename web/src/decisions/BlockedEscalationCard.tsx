import type { BlockedEscalation } from "../api";

/**
 * Blocks the operator asked to hear about directly.
 *
 * They reported "we have workers with blocked tasks and the queen never changes
 * anything". Surfacing an aged block to Queen tells the party that was already
 * silent; this reaches past her, at the twelve hours they chose over the
 * recommended twenty-four.
 *
 * IT REPORTS AND DOES NOTHING ELSE. Queen stays the actor who moves work out of
 * Blocked — the operator asked not to lose the arbitrator design — so there is
 * no unblock button here, deliberately.
 */
export default function BlockedEscalationCard({ escalations, onOpenTask }: {
  escalations: BlockedEscalation[];
  onOpenTask?: (taskId: string) => void;
}) {
  if (escalations.length === 0) return null;
  return (
    <section className="apiary-attention-card blocked-escalation-card" aria-labelledby="blocked-escalation-heading">
      {/* ONE child of the card grid, not four. The card lays its children out in
          columns, so flat children land in an icon slot and an action slot and
          wrap to nothing — which is exactly how this first shipped: the eyebrow
          set one letter per line in a 44px column. */}
      <div>
        <p className="eyebrow">Waiting on someone</p>
        <h3 id="blocked-escalation-heading">
          {escalations.length === 1
            ? "A task has been blocked more than 12 hours"
            : `${escalations.length} tasks have been blocked more than 12 hours`}
        </h3>
        <ul className="blocked-escalation-list">
          {escalations.map((escalation) => (
            <li key={escalation.task_id}>
              <button
                type="button"
                className="blocked-escalation-title"
                onClick={() => onOpenTask?.(escalation.task_id)}
              >
                {escalation.title}
              </button>
              <span className="blocked-escalation-meta">
                {escalation.worker_name} · {formatWaited(escalation.blocked_for_seconds)}
              </span>
            </li>
          ))}
        </ul>
        <p>Queen moves work out of Blocked. This is here so a stall is not silent.</p>
      </div>
    </section>
  );
}

/**
 * How long, in the coarsest unit that is still true.
 *
 * Rounded DOWN, so a block never claims to have waited longer than it has —
 * this number is the whole argument for interrupting someone.
 */
function formatWaited(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  if (hours < 48) return `${hours} hours`;
  return `${Math.floor(hours / 24)} days`;
}
