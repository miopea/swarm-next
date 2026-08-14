import { useEffect, useState, type FormEvent } from "react";

import type { JiraComment } from "../api";

export default function JiraDiscussion({ taskId, issueKey, onFetch, onAdd }: {
  taskId: string;
  issueKey: string;
  onFetch: (taskId: string) => Promise<JiraComment[]>;
  onAdd: (taskId: string, body: string) => Promise<{ state: string }>;
}) {
  const [comments, setComments] = useState<JiraComment[]>([]);
  const [commentBody, setCommentBody] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    void loadComments();
  }, [taskId]); // The task identity is the lifecycle boundary for a discussion.

  async function loadComments() {
    setLoading(true);
    setError("");
    try {
      setComments(await onFetch(taskId));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Jira discussion is unavailable.");
    } finally {
      setLoading(false);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = commentBody.trim();
    if (!body) return;
    setLoading(true);
    setError("");
    try {
      const result = await onAdd(taskId, body);
      setCommentBody("");
      setComments(await onFetch(taskId));
      setMessage(result.state === "delivered" ? "Shared to Jira." : "Saved safely; Jira delivery is pending.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Jira update could not be sent.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="jira-discussion" aria-label={`Jira discussion for ${issueKey}`}>
      <div className="jira-discussion-heading"><strong>Jira discussion</strong><small>Two-way · shared with everyone on the issue</small></div>
      {loading && comments.length === 0 ? <p>Loading discussion…</p> : null}
      {error ? <p className="settings-error" role="alert">{error}</p> : null}
      {message ? <p className="settings-message" role="status">{message}</p> : null}
      {comments.length > 0 ? (
        <ol>
          {comments.map((comment) => (
            <li key={comment.id}><span><strong>{comment.author_name}</strong><small>{comment.body}</small></span><time>{new Date(comment.created_at).toLocaleString()}</time></li>
          ))}
        </ol>
      ) : !loading ? <p>No Jira comments yet.</p> : null}
      <form onSubmit={(event) => void submit(event)}>
        <label htmlFor={`jira-comment-${taskId}`}>Add an update</label>
        <textarea id={`jira-comment-${taskId}`} value={commentBody} maxLength={4000} placeholder="Progress, a question, evidence, or a handoff" onChange={(event) => setCommentBody(event.target.value)} />
        <button className="secondary-button" type="submit" disabled={loading || !commentBody.trim()}>Share to Jira</button>
      </form>
    </section>
  );
}
