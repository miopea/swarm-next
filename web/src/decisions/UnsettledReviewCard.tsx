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
 *
 * WHAT THE FIRST VERSION GOT WRONG, since the rebuild is otherwise unexplained.
 * It shipped in v1.1.0 and the operator's verdict on eleven real rows was "no
 * clear which worker ... I cannot scan it to know what i needed". Three
 * separate faults, none of which a three-row fixture with short titles could
 * produce:
 *
 *   - The row never carried a worker, so the card could not answer the first
 *     question asked of it.
 *   - Title and reason were two variable-width columns in a wrapping flex row,
 *     so a long title pushed the reason onto a second line. Five of eleven
 *     wrapped, rows were alternately one and two lines, and the eye had no
 *     column to run down.
 *   - The reason was a sentence repeated per row. Seven of eleven said the same
 *     forty-eight characters. The majority of the ink was three strings, and
 *     the titles — the only part that differed — competed with them for space.
 *
 * So: grouped by worker, one line per row, and the sentence said once at the
 * bottom instead of eleven times up the list.
 */

/**
 * The chip for each state, and it is deliberately not the sentence shortened.
 *
 * A chip is read at a glance and a sentence is read once, so they are allowed
 * to be worded differently. What they may not do is disagree, which is why the
 * server picks the label and the sentence together in one place.
 */
const KIND_LABELS: Record<string, string> = {
  claim_unapproved: "Claim unapproved",
  code_no_deployment: "Code, no deploy",
  nothing_reported: "Nothing reported",
  settling: "Settling",
};

/**
 * A state the server has added and this build has no chip for.
 *
 * Shown as the raw label rather than dropped: an unlabelled row is still work
 * waiting on the operator, and hiding it would make the card's count disagree
 * with its own list.
 */
function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

/**
 * The rows in display order, gathered under their worker.
 *
 * The server already sorts by worker and then by age, so this only has to
 * gather runs — it does not re-sort, and it must not, or the two would be free
 * to disagree about what "oldest first" means.
 */
export function groupedByWorker(waiting: UnsettledReview[]): {
  worker: string;
  rows: UnsettledReview[];
}[] {
  const groups: { worker: string; rows: UnsettledReview[] }[] = [];
  for (const row of waiting) {
    const last = groups.at(-1);
    if (last && last.worker === row.worker_name) last.rows.push(row);
    else groups.push({ worker: row.worker_name, rows: [row] });
  }
  return groups;
}

/**
 * Each state present, once, with the sentence that explains it.
 *
 * Only the states actually on the list: a legend for a state nobody has is a
 * line of text explaining nothing, and this card is already too dense.
 */
export function legendFor(waiting: UnsettledReview[]): { kind: string; reason: string }[] {
  const seen = new Map<string, string>();
  for (const row of waiting) if (!seen.has(row.kind)) seen.set(row.kind, row.reason);
  return [...seen].map(([kind, reason]) => ({ kind, reason }));
}

export default function UnsettledReviewCard({ waiting, onOpenTask }: {
  waiting: UnsettledReview[];
  onOpenTask?: (taskId: string) => void;
}) {
  if (waiting.length === 0) return null;
  const groups = groupedByWorker(waiting);
  return (
    <section className="apiary-attention-card unsettled-review-card" aria-labelledby="unsettled-review-heading">
      {/* ONE child of the card grid. Flat children land in the icon and action
          columns and wrap to nothing — the mistake the blocked-escalation card
          shipped with and documents. */}
      <div>
        <p className="eyebrow">Nothing has settled these</p>
        {/* WAITING ON QUEEN, NOT ON YOU. The heading said "waiting on you" to
            the operator, and the operator ruled otherwise: "code, no deploy and
            nothing reported is the QUEEN'S job to find out why, keep the workers
            moving. She is managing the workers and workers who didn't do their
            job and are sitting idle is her job to find out why."

            None of the three states is the operator's to clear. An unapproved
            claim is approved with swarm_approve_no_deployment; work nobody
            reported goes back to the worker; commits with no deployment get a
            deploy task routed to the worker that owns the repository. All three
            are Queen's, so addressing the operator turned her backlog into
            their personal to-do list — and named the wrong person as the reason
            it is not moving. */}
        <h3 id="unsettled-review-heading">
          {waiting.length === 1
            ? "1 piece of finished work is waiting on Queen"
            : `${waiting.length} pieces of finished work are waiting on Queen`}
        </h3>
        {groups.map((group) => (
          <section className="unsettled-review-group" key={group.worker}>
            <h4>
              {group.worker}
              <span className="unsettled-review-group-count">{group.rows.length}</span>
            </h4>
            <ul className="unsettled-review-list">
              {group.rows.map((task) => (
                <li key={task.task_id}>
                  {/* The full title in `title`, because the visible one is
                      clipped to hold the row to a single line. Clipping is what
                      makes eleven rows scan; losing the text would not be. */}
                  <button
                    type="button"
                    className="unsettled-review-title"
                    title={task.title}
                    onClick={() => onOpenTask?.(task.task_id)}
                  >
                    {task.title}
                  </button>
                  <span
                    className={`unsettled-review-kind kind-${task.kind}`}
                    title={task.reason}
                  >
                    {kindLabel(task.kind)}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}
        <dl className="unsettled-review-legend">
          {legendFor(waiting).map((entry) => (
            <div key={entry.kind}>
              <dt className={`unsettled-review-kind kind-${entry.kind}`}>{kindLabel(entry.kind)}</dt>
              <dd>{entry.reason}</dd>
            </div>
          ))}
        </dl>
        {/* Kept word for word. It reads as loose body copy and it says the one
            thing that stops this being mistaken for a queue automation should
            have emptied — so it becomes a footnote rather than a deletion. */}
        <p className="unsettled-review-footnote">
          Work that shipped, and work whose commits show there was nothing to ship, closes on its
          own. This is what is left.
        </p>
      </div>
    </section>
  );
}
