import { useRef, useState } from "react";
import { fetchSupportStatus, submitSupport, retrySupport, forgetSupportCopy, type SupportDelivery, type SupportStatus, type SupportSubmission } from "../api/support";
import { useModalFocus } from "../shared/useModalFocus";
import { RuntimeRequestError } from "../api/request";
import UnsavedChangesPrompt from "../shared/UnsavedChangesPrompt";
import { clearPendingSupport, loadPendingSupport, savePendingSupport, prepareSupportRetry, clearSupportRetry } from "./supportDraft";

type Props = { operatorToken: string; status: SupportStatus; onClose: () => void; onSaved?: () => void };
const labels: Record<SupportDelivery["delivery"]["state"], string> = {
  pending: "Saved to Hive — waiting to send", delivering: "Sending to Swarm Support",
  uncertain: "Delivery unconfirmed — original report retained", failed: "Delivery failed — report retained",
  confirmed: "Received by Swarm Support",
  rate_limited: "Support is busy — original report retained",
};

function deliveryLabel(row: SupportDelivery) {
  if (row.delivery.refusal === "conflict") return "Support refused a conflicting report key — original report retained";
  if (row.delivery.refusal === "rejected") return "Support refused this report — original report retained";
  if (row.delivery.refusal === "rate_limited") {
    if (row.delivery.attempts >= 5) return "Support is busy — automatic retry limit reached; report retained";
    if (row.delivery.retry_not_before) return `Support is busy — retry no earlier than ${new Date(row.delivery.retry_not_before * 1000).toLocaleString()}`;
    return "Support is busy — retry time unavailable; report retained for manual retry";
  }
  return labels[row.delivery.state];
}

export default function SupportFeedbackDialog({ operatorToken, status: initial, onClose, onSaved }: Props) {
  const [recovery] = useState(() => {
    try { return { pending: loadPendingSupport(), error: "" }; }
    catch (error) { return { pending: undefined, error: String(error) }; }
  });
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<SupportSubmission["kind"]>("bug_report");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [review, setReview] = useState(recovery.pending);
  const [attempted, setAttempted] = useState(Boolean(recovery.pending));
  const [status, setStatus] = useState(initial);
  const [saved, setSaved] = useState<SupportDelivery>();
  const [error, setError] = useState(recovery.error);
  const [busy, setBusy] = useState(false);
  const [discard, setDiscard] = useState(false);
  const [removing, setRemoving] = useState<string>();
  const [visibleCount, setVisibleCount] = useState(10);
  const inFlight = useRef(false);
  const dirty = !saved && Boolean(email || name || subject || body || review);
  function close() { if (dirty && !attempted) setDiscard(true); else onClose(); }
  const modal = useModalFocus<HTMLElement>(close);

  async function send() {
    if (!review || inFlight.current) return;
    inFlight.current = true; setBusy(true); setError("");
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 15_000);
    try {
      // If browser persistence fails, do not risk an unrecoverable duplicate.
      savePendingSupport(review);
      setAttempted(true);
      const result = await submitSupport(operatorToken, review, controller.signal);
      setSaved(result);
      setStatus((old) => ({ ...old, deliveries: [result, ...old.deliveries.filter((row) => row.submission_key !== result.submission_key)] }));
      try { clearPendingSupport(); } catch { /* exact replay remains safe if browser cleanup fails */ }
      onSaved?.();
    } catch {
      setError("The save could not be confirmed. Keep this exact report and retry; a retry will not create a second report.");
    } finally { window.clearTimeout(deadline); inFlight.current = false; setBusy(false); }
  }

  async function refresh() {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true);
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    try {
      const next = await fetchSupportStatus(operatorToken, controller.signal);
      setStatus(next); setError("");
      const known = next.deliveries.find((row) => row.submission_key === review?.submission_key);
      if (known) { setSaved(known); try { clearPendingSupport(); } catch { /* retained exact key is safe */ } }
    } catch { setError("Delivery status is unavailable. Your original report is retained."); }
    finally { window.clearTimeout(deadline); inFlight.current = false; setBusy(false); }
  }

  async function retryOnce(row: SupportDelivery) {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError("");
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 15_000);
    try {
      const command = prepareSupportRetry(row);
      const delivery = await retrySupport(operatorToken, command, controller.signal);
      clearSupportRetry();
      setStatus((old) => ({ ...old, deliveries: old.deliveries.map((item) => item.submission_key === row.submission_key
        ? { ...item, delivery } : item) }));
    } catch (error) {
      if (error instanceof RuntimeRequestError && [400, 404, 409, 422].includes(error.status)) {
        try { clearSupportRetry(); } catch { /* preserve an unreadable local command for explicit recovery */ }
        setError("That retry was refused because delivery changed. Check delivery status before trying again.");
      } else { setError("Retry could not be confirmed. Try this same report again to recover the original retry request."); }
    }
    finally { window.clearTimeout(deadline); inFlight.current = false; setBusy(false); }
  }

  async function removeLocal(row: SupportDelivery) {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError("");
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    try {
      await forgetSupportCopy(operatorToken, row, controller.signal);
      setStatus((old) => ({ ...old, deliveries: old.deliveries.filter((item) => item.submission_key !== row.submission_key) }));
      setRemoving(undefined);
    } catch { setError("Local removal could not be confirmed. Check delivery status or repeat this same removal. The central conversation is unchanged."); }
    finally { window.clearTimeout(deadline); inFlight.current = false; setBusy(false); }
  }

  return <div className="feedback-backdrop" role="presentation">
    <section ref={modal} tabIndex={-1} className="feedback-dialog support-feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="support-heading">
      <header><div><p className="eyebrow">A note to the hive keepers</p><h2 id="support-heading">Swarm Support</h2></div>
        <button type="button" className="secondary-button" onClick={close}>Close</button></header>
      <p>Share feedback privately with Swarm Support. Only an email address is required for contact. No GitHub account is needed.</p>
      {!status.configured && <p role="status">Central support is disabled. Existing reports are retained; new reports cannot be sent here.</p>}
      {!review && !recovery.error && status.configured ? <form onSubmit={(event) => { event.preventDefault(); setReview({
        submission_key: crypto.randomUUID(), kind, email, name: name || null, subject, body,
      }); }}>
        <div className="feedback-fields">
          <label>Email<input type="email" required maxLength={320} value={email} onChange={(event) => setEmail(event.target.value)} /></label>
          <label>Name (optional)<input maxLength={200} value={name} onChange={(event) => setName(event.target.value)} /></label>
          <label>Type<select value={kind} onChange={(event) => setKind(event.target.value as SupportSubmission["kind"])}>
            <option value="bug_report">Bug report</option><option value="feature_request">Feature request</option><option value="feedback">Feedback</option>
          </select></label>
          <label>Subject<input required maxLength={240} value={subject} onChange={(event) => setSubject(event.target.value)} /></label>
          <label className="support-message-field">Message<textarea required maxLength={20000} value={body} onChange={(event) => setBody(event.target.value)} /></label>
        </div><button className="primary-action" type="submit" disabled={!email.trim() || !subject.trim() || !body.trim()}>Review message</button>
      </form> : review ? <section aria-label="Review support message">
        <h3>{review.subject}</h3><p>{review.name ? `${review.name} · ` : ""}{review.email}</p>
        <pre className="support-review-text">{review.body}</pre>
        {saved ? <p role="status">{deliveryLabel(saved)}</p> : <div className="diagnostic-actions">
          {!attempted && <button type="button" className="secondary-button" onClick={() => setReview(undefined)}>Edit message</button>}
          <button type="button" className="primary-action" disabled={busy || !status.configured} onClick={() => void send()}>
            {busy ? "Saving…" : attempted ? "Retry this exact report" : "Send to Swarm Support"}</button>
        </div>}
      </section> : null}
      <small className="privacy-note">Only the reviewed message and contact details are sent. Swarm Support may reply by email. Attachments and automatic diagnostic uploads are not available here.</small>
      <small className="privacy-note">An unsent retry copy stays in this tab until the Hive confirms saving it. Reopening never sends it automatically.</small>
      {error && <p role="alert">{error}</p>}
      {status.sender === "failed" && <p role="alert">Support delivery has stopped. Saved reports remain on this Hive.</p>}
      <details><summary>Delivery status · {status.deliveries.length}</summary>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => void refresh()}>Check delivery status</button>
        <ul>{status.deliveries.slice(0, visibleCount).map((row) => <li key={row.submission_key}>Report …{row.submission_key.slice(-8)} · {deliveryLabel(row)} · {new Date(row.created_at * 1000).toLocaleString()}
          {row.delivery.manual_retry_pending ? <span> · One retry requested</span> : status.configured && row.delivery.attempt_id
            && (row.delivery.state === "failed" || (["uncertain", "rate_limited"].includes(row.delivery.state) && row.delivery.attempts >= 5))
            ? <button type="button" className="secondary-button" disabled={busy} onClick={() => void retryOnce(row)}>Retry once</button> : null}
          {row.delivery.state === "confirmed" && row.delivery.receipt?.message_id && (removing === row.submission_key
            ? <div><p>Remove this confirmed copy from this Hive? Its conversation and history remain in Swarm Support. The local copy cannot be restored here.</p>
              <button type="button" className="secondary-button" disabled={busy} onClick={() => void removeLocal(row)}>Remove local copy</button>
              <button type="button" className="secondary-button" disabled={busy} onClick={() => setRemoving(undefined)}>Keep copy</button></div>
            : <button type="button" className="secondary-button" disabled={busy} onClick={() => setRemoving(row.submission_key)}>Remove from this Hive…</button>)}
        </li>)}</ul>
        {status.deliveries.length > visibleCount && <button type="button" className="secondary-button" onClick={() => setVisibleCount((count) => Math.min(count + 10, 256))}>Show more reports</button>}
      </details>
      {discard && <UnsavedChangesPrompt label="Discard this message?" description="This draft has not been saved or sent." discardLabel="Discard message" onDiscard={onClose} onKeep={() => setDiscard(false)} />}
    </section>
  </div>;
}
