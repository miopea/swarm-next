import type { GithubReviewOwed } from "../api";
import BeeMascot from "../brand/BeeMascot";

/**
 * Reviews GitHub still asks of the operator, whose notes are written
 * (ADR 0175). ONE CARD EACH, like the Jira resolutions beside them.
 *
 * A teammate's review request became a task, a worker wrote notes, and the
 * task completed at once because notes build nothing to deploy. The review the
 * operator still owed went with it, and a pull request was merged without one.
 * The task still completes; the review waits here.
 *
 * THERE IS NO BUTTON TO CLEAR IT. GitHub decides when a review is no longer
 * owed: the operator submits it, the request is withdrawn, or the pull request
 * is merged or closed. Swarm notices within about five minutes. A Done button
 * here would let the board say a review happened when GitHub says it did not.
 */
export default function GithubReviewAttentionCard({ owed, onOpenTask }: {
  owed: GithubReviewOwed[];
  onOpenTask: (taskId: string) => void;
}) {
  if (owed.length === 0) return null;
  return (
    <>
      {owed.map((item) => {
        const heading = `github-review-owed-${item.task_id}`;
        const prefix = `Review ${item.reference}: `;
        const title = item.title.startsWith(prefix) ? item.title.slice(prefix.length) : item.title;
        // Built by the server from the repository and number, so it is always
        // GitHub's own link. Checked anyway: it becomes something to click.
        const link = item.url.startsWith("https://github.com/") ? item.url : null;
        return (
          <section key={item.task_id} className="queen-attention-card" aria-labelledby={heading}>
            <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
            <div>
              <p className="eyebrow">GitHub · your review</p>
              <h3 id={heading}>Review {item.reference} on GitHub</h3>
              <p>
                “{title}”. {item.requested_by ?? "Someone"} asked for your review
                {item.worker_name ? `, and ${item.worker_name} wrote notes for you.` : ", and the notes are written."}
              </p>
              {item.notes ? (
                <blockquote className="unanswered-email-draft" aria-label="The worker's review notes">{item.notes}</blockquote>
              ) : null}
              <p className="muted">This clears by itself once you submit your review on GitHub, or the request is withdrawn.</p>
            </div>
            <div className="queen-attention-actions">
              {link ? (
                <a className="primary-action" href={link} target="_blank" rel="noreferrer noopener">
                  Open on GitHub
                </a>
              ) : null}
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
