import { useEffect, useState, type FormEvent } from "react";

import {
  fetchEmailReply,
  fetchTaskDeployments,
  prepareEmailReply,
  recordTaskDeployment,
  retryEmailReply,
  sendEmailReply,
  updateEmailReplyDraft,
  type EmailReply,
  type EmailTaskSource,
  type Task,
  type TaskDeployment,
} from "../api";

type Props = { operatorToken: string; task: Task; sources: EmailTaskSource[] };

export default function EmailResolutionPanel({ operatorToken, task, sources }: Props) {
  const [deployments, setDeployments] = useState<TaskDeployment[]>([]);
  const [reply, setReply] = useState<EmailReply | null>(null);
  const [environment, setEnvironment] = useState("production");
  const [reference, setReference] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [detailsState, setDetailsState] = useState<"checking" | "ready" | "partial">("checking");
  const [confirmingSend, setConfirmingSend] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    if (task.state !== "completed") return;
    let cancelled = false;
    void loadDetails(() => cancelled);
    return () => { cancelled = true; };
  }, [operatorToken, task.id, task.state]);

  async function loadDetails(cancelled: () => boolean = () => false) {
    setBusy(true);
    setDetailsState("checking");
    const [deploymentResult, replyResult] = await Promise.allSettled([
      fetchTaskDeployments(operatorToken, task.id),
      fetchEmailReply(operatorToken, task.id),
    ]);
    if (cancelled()) return;
    if (deploymentResult.status === "fulfilled") setDeployments(deploymentResult.value);
    if (replyResult.status === "fulfilled") {
      setReply(replyResult.value);
      setBody(replyResult.value?.body ?? "");
    }
    setDetailsState(deploymentResult.status === "fulfilled" && replyResult.status === "fulfilled" ? "ready" : "partial");
    setBusy(false);
  }

  async function recordDeployment(event: FormEvent) {
    event.preventDefault();
    if (!reference.trim()) return;
    setBusy(true);
    setMessage("");
    try {
      const deployment = await recordTaskDeployment(operatorToken, task.id, environment, reference);
      setDeployments((current) => [deployment, ...current]);
      setReference("");
      setMessage("Deployment recorded. You can now prepare the customer-facing reply.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Deployment evidence could not be recorded.");
    } finally {
      setBusy(false);
    }
  }

  async function saveReply() {
    if (!body.trim()) return;
    setBusy(true);
    setMessage("");
    try {
      const saved = reply
        ? await updateEmailReplyDraft(operatorToken, task.id, body)
        : await prepareEmailReply(operatorToken, task.id, body);
      setReply(saved);
      setBody(saved.body);
      setMessage(reply ? "Reply changes saved for review." : "Reply saved for review. Nothing has been sent.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "The reply could not be saved.");
    } finally {
      setBusy(false);
    }
  }

  async function sendReply() {
    if (!reply) return;
    setBusy(true);
    setMessage("");
    try {
      const sent = await sendEmailReply(operatorToken, reply.id);
      setReply(sent);
      setConfirmingSend(false);
      setMessage(replyStateMessage(sent));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "The reply could not be sent.");
    } finally {
      setBusy(false);
    }
  }

  async function retryUncertain() {
    if (!reply) return;
    setBusy(true);
    setMessage("");
    try {
      const retried = await retryEmailReply(operatorToken, reply.id);
      setReply(retried);
      setMessage(replyStateMessage(retried));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "The reply could not be retried.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="email-resolution-panel" aria-label={`Email details for ${task.title}`}>
      <div className="email-resolution-sources">
        <span className="field-caption">{sources.length === 1 ? "Original report" : `${sources.length} original reports`}</span>
        <ul aria-label="Original email threads">
          {sources.map((source) => (
            <li key={source.id}><span><strong>{source.sender_name || source.sender_address}</strong><small>{source.sender_address}</small></span><a href={source.web_url} target="_blank" rel="noreferrer">Open email</a></li>
          ))}
        </ul>
      </div>
      {sources.some((source) => source.attachments.length > 0) ? (
        <div className="email-resolution-attachments">
          <span className="field-caption">Original attachments</span>
          {sources.flatMap((source) => source.attachments.map((attachment) => (
            <a key={`${source.id}:${attachment.storage_name}`} href={`/api/v1/tasks/${encodeURIComponent(task.id)}/email/attachments/${encodeURIComponent(attachment.storage_name)}`} download={attachment.display_name}>{attachment.display_name}</a>
          )))}
        </div>
      ) : null}
      {task.state !== "completed" ? (
        <p className="email-resolution-note">The original thread stays linked. A reply becomes available only after this task is completed and its deployment is recorded.</p>
      ) : detailsState === "checking" ? (
        <p className="email-resolution-note" role="status">Checking deployment and reply history…</p>
      ) : detailsState === "partial" ? (
        <div className="settings-error" role="alert"><span>Swarm could not verify the complete email history. No deployment or reply action is available until both records are known.</span> <button className="text-button" type="button" disabled={busy} onClick={() => void loadDetails()}>Retry completion details</button></div>
      ) : deployments.length === 0 ? (
        <form className="email-deployment-form" onSubmit={(event) => void recordDeployment(event)}>
          <div><span className="field-caption">Step 1 of 2</span><strong>Confirm the fix is live</strong><small>Deployment evidence prevents a completion status from emailing someone before the change is actually available.</small></div>
          <label><span>Environment</span><select value={environment} onChange={(event) => setEnvironment(event.target.value)}><option value="production">Production</option><option value="staging">Staging</option><option value="other">Other</option></select></label>
          <label><span>Release, URL, or deployment reference</span><input value={reference} placeholder="Release 2026.8.14 or https://…" onChange={(event) => setReference(event.target.value)} /></label>
          <button className="secondary-button" type="submit" disabled={busy || !reference.trim()}>Record deployment</button>
        </form>
      ) : (
        <div className="email-reply-workflow">
          <div className="email-deployment-proof"><span>Deployed</span><strong>{deployments[0].environment}</strong><small>{deployments[0].reference}</small></div>
          {reply?.state === "delivered" ? (
            <div className="email-reply-delivered" role="status"><strong>Replies delivered to {reply.targets.length} {reply.targets.length === 1 ? "thread" : "threads"}</strong><p>{reply.body}</p><ReplyTargets reply={reply} /></div>
          ) : reply?.state === "uncertain" ? (
            <div className="email-reply-uncertain" role="alert"><strong>One or more deliveries could not be confirmed</strong><p>Swarm will not retry an uncertain thread automatically because Outlook may already have accepted it. Check every uncertain thread first.</p><ReplyTargets reply={reply} /><button className="secondary-button" type="button" disabled={busy} onClick={() => void retryUncertain()}>I checked uncertain threads · retry</button></div>
          ) : reply && ["queued", "dispatching"].includes(reply.state) ? (
            <div className="email-reply-sending" role="status"><strong>Replies are being delivered</strong><p>Each original thread has its own durable, idempotent delivery. Swarm will not create duplicate queued copies.</p><ReplyTargets reply={reply} /></div>
          ) : (
            <>
              <label className="email-reply-editor"><span><span className="field-caption">Step 2 of 2</span> Plain-language resolution</span><textarea rows={5} value={body} placeholder="Thank you for reporting this. We fixed…" onChange={(event) => { setBody(event.target.value); setConfirmingSend(false); }} /></label>
              <small className="privacy-note">Write for the person who reported the issue: what changed, what they can do now, and no internal implementation details.</small>
              <div className="email-reply-actions">
                <button className="secondary-button" type="button" disabled={busy || !body.trim() || reply?.body === body.trim()} onClick={() => void saveReply()}>{reply ? "Save reply changes" : "Save reply for review"}</button>
                {reply && reply.body === body.trim() ? <button className="primary-action" type="button" disabled={busy} onClick={() => setConfirmingSend(true)}>Review and send</button> : null}
              </div>
              {confirmingSend && reply ? (
                <div className="email-send-confirmation" role="group" aria-label="Confirm email reply"><strong>Send this reply to {reply.targets.length} {reply.targets.length === 1 ? "original thread" : "original threads"}?</strong><p>{reply.body}</p><ReplyTargets reply={reply} /><span>Every listed Outlook thread receives one reply. This action cannot be undone from Swarm.</span><div className="email-reply-actions"><button className="secondary-button" type="button" onClick={() => setConfirmingSend(false)}>Keep editing</button><button className="primary-action" type="button" disabled={busy} onClick={() => void sendReply()}>Send {reply.targets.length === 1 ? "reply" : `${reply.targets.length} replies`} now</button></div></div>
              ) : null}
            </>
          )}
        </div>
      )}
      {message ? <p className="settings-message" role="status">{message}</p> : null}
    </section>
  );
}

function ReplyTargets({ reply }: { reply: EmailReply }) {
  return <ul className="email-reply-targets" aria-label="Email reply recipients">{reply.targets.map((target) => (
    <li key={target.id}><span><strong>{target.sender_name || target.sender_address}</strong><small>{target.sender_address}</small></span><span className={`delivery-state ${target.state}`}>{replyTargetStateLabel(target.state)}</span></li>
  ))}</ul>;
}

function replyStateMessage(reply: EmailReply) {
  const count = reply.targets.length;
  if (reply.state === "delivered") return `${count === 1 ? "Reply" : `${count} replies`} delivered to the original Outlook ${count === 1 ? "thread" : "threads"}.`;
  if (reply.state === "uncertain") return "Outlook may have accepted one or more replies, but delivery could not be confirmed.";
  return `${count === 1 ? "Reply" : `${count} replies`} queued for delivery.`;
}

function replyTargetStateLabel(state: EmailReply["targets"][number]["state"]) {
  return ({
    draft: "Draft",
    queued: "Ready to send",
    dispatching: "Sending",
    delivered: "Delivered",
    uncertain: "Needs review",
    cancelled: "Stopped",
  } as const)[state];
}
