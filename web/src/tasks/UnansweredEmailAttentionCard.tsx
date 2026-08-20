import type { UnansweredEmailTask } from "../api";
import BeeMascot from "../brand/BeeMascot";

/**
 * Completed email tasks whose requester has not been answered.
 *
 * A task imported from email carries a person waiting on it, and finishing the
 * work tells them nothing: the reply is a separate, deliberate step. Nothing
 * used to notice when that step never happened, so a worker could close the
 * task and the person who wrote in heard nothing at all.
 *
 * This reports the silence. It does not send anything — the reply is written
 * and reviewed on the task itself, where the original thread and the
 * deployment evidence are.
 */
export default function UnansweredEmailAttentionCard({ awaiting, onOpenTask }: {
  awaiting: UnansweredEmailTask[];
  onOpenTask: (taskId: string) => void;
}) {
  if (awaiting.length === 0) return null;
  const [first] = awaiting;
  return (
    <section className="queen-attention-card" aria-labelledby="unanswered-email-heading">
      <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
      <div>
        <p className="eyebrow">Email</p>
        <h3 id="unanswered-email-heading">
          {awaiting.length === 1 ? "One finished task has not been answered" : `${awaiting.length} finished tasks have not been answered`}
        </h3>
        <p>
          {first.sender_name || first.sender_address} is still waiting on “{first.title}”
          {awaiting.length > 1 ? `, and ${awaiting.length - 1} other${awaiting.length === 2 ? "" : "s"} like it` : ""}.
          {first.drafted ? " A reply is written but was never sent." : " No reply has been written."}
        </p>
      </div>
      <div className="queen-attention-actions">
        <button className="primary-action" type="button" onClick={() => onOpenTask(first.task_id)}>
          {awaiting.length === 1 ? "Open the task" : "Open the oldest"}
        </button>
      </div>
    </section>
  );
}
