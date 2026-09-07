import { useRef, useState } from "react";
import { fetchSupportStatus, submitSupport, type SupportDelivery, type SupportStatus, type SupportSubmission } from "../api/support";
import { useModalFocus } from "../shared/useModalFocus";
import UnsavedChangesPrompt from "../shared/UnsavedChangesPrompt";
import { clearPendingSupport, loadPendingSupport, savePendingSupport } from "./supportDraft";

type Props = { operatorToken: string; status: SupportStatus; onClose: () => void; onSaved?: () => void };
const labels: Record<SupportDelivery["delivery"]["state"], string> = {
  pending: "Saved to Hive — waiting to send", delivering: "Sending to Swarm Support",
  uncertain: "Delivery unconfirmed — original report retained", failed: "Delivery failed — report retained",
  confirmed: "Received by Swarm Support",
};

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

  return <div className="feedback-backdrop" role="presentation">
    <section ref={modal} tabIndex={-1} className="feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="support-heading">
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
        <pre className="diagnostic-preview">{review.body}</pre>
        {saved ? <p role="status">{labels[saved.delivery.state]}</p> : <div className="diagnostic-actions">
          {!attempted && <button type="button" className="secondary-button" onClick={() => setReview(undefined)}>Edit message</button>}
          <button type="button" className="primary-action" disabled={busy || !status.configured} onClick={() => void send()}>
            {busy ? "Saving…" : attempted ? "Retry this exact report" : "Send to Swarm Support"}</button>
        </div>}
      </section> : null}
      <small className="privacy-note">Only the reviewed message and contact details are sent. Attachments, diagnostics, and reply delivery are not enabled in this integration yet.</small>
      <small className="privacy-note">An unsent retry copy stays in this tab until the Hive confirms saving it. Reopening never sends it automatically.</small>
      {error && <p role="alert">{error}</p>}
      {status.sender === "failed" && <p role="alert">Support delivery has stopped. Saved reports remain on this Hive.</p>}
      <details><summary>Delivery status · {status.deliveries.length}</summary>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => void refresh()}>Check delivery status</button>
        <ul>{status.deliveries.slice(0, 10).map((row) => <li key={row.submission_key}>{labels[row.delivery.state]} · {new Date(row.created_at * 1000).toLocaleString()}</li>)}</ul>
      </details>
      {discard && <UnsavedChangesPrompt label="Discard this message?" description="This draft has not been saved or sent." discardLabel="Discard message" onDiscard={onClose} onKeep={() => setDiscard(false)} />}
    </section>
  </div>;
}
