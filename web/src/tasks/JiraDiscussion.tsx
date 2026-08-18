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
    } catch {
      setError("Jira discussion could not be loaded. Your task and any typed update are still safe.");
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
      setMessage(result.state === "delivered" ? "Shared to Jira." : "Saved safely; Jira delivery is pending.");
      try {
        setComments(await onFetch(taskId));
      } catch {
        setError("Your update was saved, but the discussion could not refresh. Retry the discussion instead of posting it again.");
      }
    } catch {
      setError("The Jira update could not be saved. It remains in the editor so you can retry.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="jira-discussion" aria-label={`Jira discussion for ${issueKey}`}>
      <div className="jira-discussion-heading"><strong>Jira discussion</strong><small>Two-way · shared with everyone on the issue</small></div>
      {loading && comments.length === 0 ? <p>Loading discussion…</p> : null}
      {error ? <div className="settings-error" role="alert"><span>{error}</span> <button className="text-button" type="button" disabled={loading} onClick={() => void loadComments()}>Retry discussion</button></div> : null}
      {message ? <p className="settings-message" role="status">{message}</p> : null}
      {comments.length > 0 ? (
        <ol>
          {comments.map((comment) => (
            <li key={comment.id}><span><strong>{comment.author_name}</strong><small>{comment.body}</small></span><time>{new Date(comment.created_at).toLocaleString()}</time></li>
          ))}
        </ol>
      ) : !loading && !error ? <p>No Jira comments yet.</p> : null}
      <form onSubmit={(event) => void submit(event)}>
        <label htmlFor={`jira-comment-${taskId}`}>Add an update</label>
        <textarea id={`jira-comment-${taskId}`} value={commentBody} maxLength={4000} placeholder="Progress, a question, evidence, or a handoff" onChange={(event) => setCommentBody(event.target.value)} />
        <button className="secondary-button" type="submit" disabled={loading || !commentBody.trim()}>Share to Jira</button>
      </form>
    </section>
  );
}
