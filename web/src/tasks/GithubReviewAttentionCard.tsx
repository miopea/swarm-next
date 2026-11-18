import { useState } from "react";

import type { GithubReviewOwed, GithubReviewSubmitted, GithubReviewVerdict } from "../api";
import BeeMascot from "../brand/BeeMascot";

/** Where a press ended up, kept after the review leaves Needs you. */
export type GithubReviewResult = GithubReviewSubmitted & { task_id: string; reference: string };

/** A press still being carried out: the buttons wait for it. */
const IN_FLIGHT = new Set(["queued", "posting", "post_uncertain"]);

/**
 * Reviews GitHub still asks of the operator, whose notes are written
 * (ADR 0175). ONE CARD EACH, like the Jira resolutions beside them.
 *
 * THE OPERATOR SUBMITS FROM HERE (ADR 0176). The worker read the pull request
 * and proposed a verdict and a comment to the author. The card shows exactly
 * what will be posted under the operator's GitHub name, lets them edit it, and
 * posts it with one press. Approve then merges when GitHub allows it, and says
 * why when it does not.
 *
 * THERE IS STILL NO DONE BUTTON. A review leaves this list because GitHub has
 * it, never because the board was told so.
 */
export default function GithubReviewAttentionCard({
  owed,
  results = [],
  busy = false,
  onSubmit,
  onDismissResult,
  onOpenTask,
}: {
  owed: GithubReviewOwed[];
  /** Presses whose review has left Needs you, until the operator dismisses them. */
  results?: GithubReviewResult[];
  busy?: boolean;
  onSubmit?: (item: GithubReviewOwed, verdict: GithubReviewVerdict, comment: string) => void;
  onDismissResult?: (taskId: string) => void;
  onOpenTask: (taskId: string) => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const stillOwed = new Set(owed.map((item) => item.task_id));
  const finished = results.filter((result) => !stillOwed.has(result.task_id));
  if (owed.length === 0 && finished.length === 0) return null;
  return (
    <>
      {owed.map((item) => {
        const heading = `github-review-owed-${item.task_id}`;
        const prefix = `Review ${item.reference}: `;
        const title = item.title.startsWith(prefix) ? item.title.slice(prefix.length) : item.title;
        // Built by the server from the repository and number, so it is always
        // GitHub's own link. Checked anyway: it becomes something to click.
        const link = item.url.startsWith("https://github.com/") ? item.url : null;
        const reader = item.worker_name ?? "The worker";
        const proposal = item.proposal;
        const comment = drafts[item.task_id] ?? proposal?.comment ?? "";
        const inFlight = item.submission ? IN_FLIGHT.has(item.submission.state) : false;
        const disabled = busy || inFlight || !onSubmit;
        const author = item.requested_by ?? "the author";
        const approve = (
          <button
            key="approve"
            className={proposal?.verdict === "approve" ? "primary-action" : "secondary-button"}
            type="button"
            disabled={disabled}
            onClick={() => onSubmit?.(item, "approve", comment)}
          >
            Approve and merge
          </button>
        );
        const requestChanges = (
          <button
            key="request-changes"
            className={proposal?.verdict === "request_changes" ? "primary-action" : "secondary-button"}
            type="button"
            disabled={disabled || comment.trim() === ""}
            title={comment.trim() === "" ? "Write what should change first" : undefined}
            onClick={() => onSubmit?.(item, "request_changes", comment)}
          >
            Request changes
          </button>
        );
        return (
          <section key={item.task_id} className="queen-attention-card github-review-card" aria-labelledby={heading}>
            <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
            <div>
              <p className="eyebrow">GitHub · your review</p>
              <h3 id={heading}>Review {item.reference}</h3>
              {proposal ? (
                <>
                  <p>
                    “{title}”. {item.requested_by ?? "Someone"} asked for your review. {reader} read it
                    and suggests you {proposal.verdict === "approve" ? "approve" : "request changes"}.
                  </p>
                  <label className="github-review-comment">
                    <span className="field-caption">Comment to {author}, posted as you</span>
                    <textarea
                      rows={5}
                      value={comment}
                      disabled={disabled}
                      onChange={(event) => {
                        const value = event.target.value;
                        setDrafts((current) => ({ ...current, [item.task_id]: value }));
                      }}
                    />
                  </label>
                  <p className="muted">
                    Approve and merge merges only if it is not a draft, nothing was pushed after {reader} read
                    it, and every check passed. Otherwise it approves and tells you why it did not merge.
                  </p>
                  {item.notes ? (
                    <details className="github-review-notes">
                      <summary>{reader}&rsquo;s notes</summary>
                      <blockquote className="unanswered-email-draft" aria-label="The worker's review notes">{item.notes}</blockquote>
                    </details>
                  ) : null}
                </>
              ) : (
                <>
                  <p>
                    “{title}”. {item.requested_by ?? "Someone"} asked for your review
                    {item.worker_name ? `, and ${item.worker_name} wrote notes for you.` : ", and the notes are written."}
                  </p>
                  {item.notes ? (
                    <blockquote className="unanswered-email-draft" aria-label="The worker's review notes">{item.notes}</blockquote>
                  ) : null}
                  <p className="muted">
                    {reader} recorded no review to submit, so review it on GitHub. This clears by itself once you
                    submit your review there, or the request is withdrawn.
                  </p>
                </>
              )}
              {item.submission ? <SubmissionStatus state={item.submission.state} outcome={item.submission.outcome} /> : null}
            </div>
            <div className="queen-attention-actions">
              {proposal ? (proposal.verdict === "approve" ? [approve, requestChanges] : [requestChanges, approve]) : null}
              {link ? (
                <a className={proposal ? "text-button" : "primary-action"} href={link} target="_blank" rel="noreferrer noopener">
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
      {finished.map((result) => {
        const heading = `github-review-result-${result.task_id}`;
        return (
          <section key={result.task_id} className="queen-attention-card github-review-card" aria-labelledby={heading}>
            <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
            <div>
              <p className="eyebrow">GitHub · your review</p>
              <h3 id={heading}>{result.reference} reviewed</h3>
              <p role="status">{result.outcome ?? "Submitted on GitHub."}</p>
            </div>
            <div className="queen-attention-actions">
              <button className="text-button" type="button" onClick={() => onDismissResult?.(result.task_id)}>
                Dismiss
              </button>
              <button className="text-button" type="button" onClick={() => onOpenTask(result.task_id)}>
                Open the task
              </button>
            </div>
          </section>
        );
      })}
    </>
  );
}

/** The last press on a review that is still owed: in progress, or why it did not go. */
function SubmissionStatus({ state, outcome }: { state: string; outcome: string | null }) {
  if (state === "failed") {
    return <p className="unanswered-email-failure" role="alert">Not submitted: {outcome ?? "GitHub did not take it."}</p>;
  }
  if (state === "queued" || state === "posting") {
    return <p className="muted" role="status">Submitting to GitHub…</p>;
  }
  return <p className="muted" role="status">{outcome ?? "Swarm is checking whether GitHub took it."}</p>;
}
