import type { UnsettledReview } from "../api";

/**
 * Finished work that nothing has settled, and why each piece waits.
 *
 * WHY THIS EXISTS AT ALL. Nineteen exemption claims sat unapproved for weeks
 * and the reason was not that they were hidden — it is that ZERO of them were
 * anywhere a person passes. Closed work is out of sight by construction, so a
 * backlog of it reads as a mess rather than as a queue. A number on a surface
 * the operator already checks is what stops that recurring.
 *
 * IT NAMES ITS SUBJECT, in the heading and not only in the code. The figure
 * this whole design began from was "49 of 355 completed tasks carry nothing
 * anyone verified", and it was really 31 — a count of something adjacent to
 * the claim, then relayed to the operator as the sharpest evidence in it. A
 * heading that says "unverified work" would inherit that ambiguity; this one
 * says what is actually true of every row, which is that nothing has settled
 * it and a person is the remaining step.
 *
 * EVERYTHING SETTLEABLE IS ALREADY GONE. The coordinator closes work carrying a
 * deployment, and work whose recorded commits show there was nothing to
 * deploy. So this list is not a chore queue that automation should have
 * emptied — it is the residue that genuinely needs judgement.
 */
export default function UnsettledReviewCard({ waiting, onOpenTask }: {
  waiting: UnsettledReview[];
  onOpenTask?: (taskId: string) => void;
}) {
  if (waiting.length === 0) return null;
  return (
    <section className="apiary-attention-card unsettled-review-card" aria-labelledby="unsettled-review-heading">
      {/* ONE child of the card grid. Flat children land in the icon and action
          columns and wrap to nothing — the mistake the blocked-escalation card
          shipped with and documents. */}
      <div>
        <p className="eyebrow">Nothing has settled these</p>
        <h3 id="unsettled-review-heading">
          {waiting.length === 1
            ? "1 piece of finished work is waiting on you"
            : `${waiting.length} pieces of finished work are waiting on you`}
        </h3>
        <ul className="unsettled-review-list">
          {waiting.map((task) => (
            <li key={task.task_id}>
              <button
                type="button"
                className="unsettled-review-title"
                onClick={() => onOpenTask?.(task.task_id)}
              >
                {task.title}
              </button>
              <span className="unsettled-review-meta">{task.reason}</span>
            </li>
          ))}
        </ul>
        <p>
          Work that shipped, and work whose commits show there was nothing to ship, closes on its
          own. This is what is left.
        </p>
      </div>
    </section>
  );
}
