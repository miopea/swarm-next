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
 * When a reply has been written, it is shown here and can be sent from here.
 * Reading the words is the operator's part; whether the work is actually
 * running in production is the worker's, recorded as deployment evidence. The
 * reply used to be reviewed on the task, which meant going and finding the task
 * to do the one thing that was genuinely theirs.
 */
export default function UnansweredEmailAttentionCard({ awaiting, busy, onOpenTask, onSendReply }: {
  awaiting: UnansweredEmailTask[];
  busy?: boolean;
  onOpenTask: (taskId: string) => void;
  onSendReply?: (replyId: string) => void;
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
          {first.draft_body ? " A reply is written and waiting for you to read it." : first.drafted ? " A reply is written but was never sent." : " No reply has been written."}
        </p>
        {first.draft_body ? (
          <blockquote className="unanswered-email-draft">{first.draft_body}</blockquote>
        ) : null}
      </div>
      <div className="queen-attention-actions">
        {first.draft_id && onSendReply ? (
          <button className="primary-action" type="button" disabled={busy} onClick={() => onSendReply(first.draft_id!)}>
            Send this reply
          </button>
        ) : null}
        <button className={first.draft_id ? "secondary-button" : "primary-action"} type="button" onClick={() => onOpenTask(first.task_id)}>
          {first.draft_id ? "Edit first" : awaiting.length === 1 ? "Open the task" : "Open the oldest"}
        </button>
      </div>
    </section>
  );
}
