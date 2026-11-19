import { useState } from "react";

import type { JiraReplyAwaiting, JiraReplyPress } from "../api";
import BeeMascot from "../brand/BeeMascot";

/** A press Jira took, kept after the reply leaves Needs you. */
export type JiraReplyResult = JiraReplyPress & { task_id: string; issue_key: string };

/** A press still being carried out: the buttons wait for it. */
const IN_FLIGHT = new Set(["queued", "dispatching"]);

/** Jira's refusal codes, in words the operator can act on. */
const JIRA_ERRORS: Record<string, string> = {
  permission_denied: "this Hive's Jira account is not allowed to comment on it",
  credentials_invalid: "Jira did not accept this Hive's credentials",
  not_configured: "Jira is not connected to this Hive",
  network_unavailable: "Jira could not be reached",
  invalid_response: "Jira answered in a way Swarm could not read",
  response_limit_exceeded: "Jira's answer was too large to read",
};

function describeJiraError(code: string | null): string {
  if (!code) return "Jira did not say why";
  return JIRA_ERRORS[code] ?? code;
}

/**
 * Replies a worker drafted for a Jira ticket, for the operator to post
 * (ADR 0177). ONE CARD EACH, like the held resolutions it stands in for.
 *
 * THE PRESS POSTS. A worker used to ask the operator to approve a reply in a
 * decision and post it afterwards, and an approved reply sat off the ticket
 * for two minutes while that relay and the next Jira pass ran. The card shows
 * exactly what will be posted, lets the operator edit it, and one press sends
 * it before the card answers. Nothing posts without that press.
 *
 * When the ticket's resolution also waits on the operator, the card offers
 * both in one press. The ticket resolves only once Jira has the reply.
 */
export default function JiraReplyAttentionCard({
  replies,
  results = [],
  busy = false,
  onPost,
  onDiscard,
  onDismissResult,
  onOpenTask,
}: {
  replies: JiraReplyAwaiting[];
  /** Presses Jira took, until the operator dismisses them. */
  results?: JiraReplyResult[];
  busy?: boolean;
  onPost?: (item: JiraReplyAwaiting, body: string, resolve: boolean) => void;
  onDiscard?: (taskId: string) => void;
  onDismissResult?: (taskId: string) => void;
  onOpenTask: (taskId: string) => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const waiting = new Set(replies.map((item) => item.task_id));
  const finished = results.filter((result) => !waiting.has(result.task_id));
  if (replies.length === 0 && finished.length === 0) return null;
  return (
    <>
      {replies.map((item) => {
        const heading = `jira-reply-${item.task_id}`;
        const body = drafts[item.task_id] ?? item.body;
        const inFlight = item.posted ? IN_FLIGHT.has(item.posted.state) : false;
        const disabled = busy || inFlight || !onPost;
        const empty = body.trim() === "";
        const author = item.worker_name ?? "The worker";
        const post = (resolve: boolean, primary: boolean) => (
          <button
            key={resolve ? "post-and-resolve" : "post"}
            className={primary ? "primary-action" : "secondary-button"}
            type="button"
            disabled={disabled || empty}
            title={empty ? "Write the reply first" : undefined}
            onClick={() => onPost?.(item, body, resolve)}
          >
            {resolve ? "Post reply and resolve" : "Post reply"}
          </button>
        );
        return (
          <section key={item.task_id} className="queen-attention-card jira-reply-card" aria-labelledby={heading}>
            <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
            <div>
              <p className="eyebrow">{item.project_name ? `${item.project_name} · Jira reply` : "Jira reply"}</p>
              <h3 id={heading}>Reply on {item.issue_key}</h3>
              <p>
                “{item.title}”. {author} drafted this reply to the person who raised it. Nothing is posted until
                you press.
              </p>
              <label className="jira-reply-body">
                <span className="field-caption">Reply posted on {item.issue_key}</span>
                <textarea
                  rows={5}
                  value={body}
                  disabled={busy || inFlight}
                  onChange={(event) => {
                    const value = event.target.value;
                    setDrafts((current) => ({ ...current, [item.task_id]: value }));
                  }}
                />
              </label>
              {item.resolution_held ? (
                <p className="muted">
                  The ticket is complete here and waits for you to resolve it in Jira. Post reply and resolve
                  resolves it once Jira has the reply.
                </p>
              ) : item.resolution_waiting ? (
                <p className="muted">
                  The ticket is complete here. Jira resolves it once this reply is posted, or once you choose not
                  to post it.
                </p>
              ) : null}
              {item.posted ? <PostedStatus item={item} /> : null}
            </div>
            <div className="queen-attention-actions">
              {item.resolution_held ? [post(true, true), post(false, false)] : post(false, true)}
              <button className="secondary-button" type="button" disabled={busy || inFlight || !onDiscard} onClick={() => onDiscard?.(item.task_id)}>
                Don&rsquo;t post
              </button>
              <button className="text-button" type="button" onClick={() => onOpenTask(item.task_id)}>
                Open the task
              </button>
            </div>
          </section>
        );
      })}
      {finished.map((result) => {
        const heading = `jira-reply-result-${result.task_id}`;
        return (
          <section key={result.task_id} className="queen-attention-card jira-reply-card" aria-labelledby={heading}>
            <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="focused" /></span>
            <div>
              <p className="eyebrow">Jira reply</p>
              <h3 id={heading}>Reply posted on {result.issue_key}</h3>
              <p role="status">Jira has the reply.{resolutionSentence(result.resolution)}</p>
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

function resolutionSentence(resolution: string | null): string {
  if (resolution === null) return "";
  if (resolution === "delivered") return " The ticket is resolved.";
  if (resolution === "queued" || resolution === "dispatching") return " Resolving it in Jira.";
  return " Jira did not resolve the ticket; the task says why.";
}

/** A press Jira has not taken yet: on its way, or why it did not go. */
function PostedStatus({ item }: { item: JiraReplyAwaiting }) {
  const posted = item.posted;
  if (!posted) return null;
  if (posted.state === "conflict") {
    return (
      <p className="unanswered-email-failure" role="alert">
        Not posted: {describeJiraError(posted.error)}. Edit it and post again, or don&rsquo;t post it.
      </p>
    );
  }
  if (posted.state === "uncertain") {
    return (
      <p className="unanswered-email-failure" role="alert">
        Swarm stopped while posting this, so it may already be on the ticket. Check {item.issue_key} before you
        post it again.
      </p>
    );
  }
  if (posted.error) {
    return <p className="muted" role="status">Not posted yet: {describeJiraError(posted.error)}. Swarm tries again by itself.</p>;
  }
  return <p className="muted" role="status">Posting to Jira…</p>;
}
