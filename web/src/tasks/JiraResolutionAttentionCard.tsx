import type { HeldJiraResolution } from "../api";
import BeeMascot from "../brand/BeeMascot";

/**
 * Finished tickets whose Jira resolution waits for the operator (ADR 0120) —
 * ONE CARD EACH, like the email replies above them.
 *
 * Every completion of a Jira-linked task used to resolve the ticket. A support
 * request whose answer lived only in Swarm went to Resolved with nothing posted
 * on it, and the requester reopened IS-10736 and IS-10688 to ask what had
 * happened. On a project set to ask, the task completes as before and the
 * ticket waits here.
 *
 * WHETHER ANYONE ANSWERED IS THE FIRST THING ON THE CARD, because resolving a
 * ticket nobody replied to is exactly the failure this exists to stop. The
 * card says so; it does not refuse, since a ticket answered by phone is the
 * operator's call.
 */
export default function JiraResolutionAttentionCard({ held, busy, onConfirm, onLeaveOpen, onOpenTask }: {
  held: HeldJiraResolution[];
  busy?: boolean;
  onConfirm: (taskId: string) => void;
  onLeaveOpen: (taskId: string) => void;
  onOpenTask: (taskId: string) => void;
}) {
  if (held.length === 0) return null;
  return (
    <>
      {held.map((item) => {
        const heading = `held-jira-resolution-${item.task_id}`;
        return (
          <section key={item.task_id} className="queen-attention-card" aria-labelledby={heading}>
            <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
            <div>
              <p className="eyebrow">{item.project_name ? `${item.project_name} · Jira` : "Jira"}</p>
              <h3 id={heading}>Resolve {item.issue_key} in Jira?</h3>
              <p>“{item.title}” is complete in Swarm. Jira will show it as resolved to the person who asked.</p>
              {item.last_reply_at === null ? (
                <p className="jira-resolution-no-reply"><strong>No reply has been posted on this ticket.</strong> Resolving it now closes it without an answer.</p>
              ) : (
                <p>A reply was last posted on {new Date(item.last_reply_at * 1000).toLocaleString()}.</p>
              )}
              {item.handoff ? (
                <blockquote className="unanswered-email-draft" aria-label="What the worker reported">{item.handoff}</blockquote>
              ) : null}
            </div>
            <div className="queen-attention-actions">
              <button className="primary-action" type="button" disabled={busy} onClick={() => onConfirm(item.task_id)}>
                Resolve in Jira
              </button>
              <button className="secondary-button" type="button" disabled={busy} onClick={() => onLeaveOpen(item.task_id)}>
                Leave open
              </button>
              <button className="text-button" type="button" onClick={() => onOpenTask(item.task_id)}>
                Open the task
              </button>
            </div>
          </section>
        );
      })}
    </>
  );
}
