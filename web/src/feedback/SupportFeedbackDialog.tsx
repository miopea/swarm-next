import { useEffect, useRef, useState } from "react";
import { fetchSupportStatus, submitSupport, submitSupportFiles, retrySupport, forgetSupportCopy, type SupportFile, type SupportDelivery, type SupportStatus, type SupportSubmission } from "../api/support";
import { useModalFocus } from "../shared/useModalFocus";
import ModalPortal from "../shared/ModalPortal";
import { RuntimeRequestError } from "../api/request";
import UnsavedChangesPrompt from "../shared/UnsavedChangesPrompt";
import { clearPendingSupport, loadPendingSupport, savePendingSupport, prepareSupportRetry, clearSupportRetry } from "./supportDraft";
import { prepareSupportFiles, savePendingSupportFiles, loadPendingSupportFiles, clearPendingSupportFiles } from "./supportFiles";
import { fetchPublicHiveProfile } from "../api";

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
  const contactEdited = useRef({ name: false, email: false });
  const [savedContact, setSavedContact] = useState(false);
  const [kind, setKind] = useState<SupportSubmission["kind"]>("bug_report");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [review, setReview] = useState(recovery.pending);
  const [files, setFiles] = useState<SupportFile[]>([]);
  const [readingFiles, setReadingFiles] = useState(false);
  const [recoveringFiles, setRecoveringFiles] = useState(typeof indexedDB !== "undefined");
  const [filesRecoveryFailed, setFilesRecoveryFailed] = useState(false);
  const [attempted, setAttempted] = useState(Boolean(recovery.pending));
  const [status, setStatus] = useState(initial);
  const [saved, setSaved] = useState<SupportDelivery>();
  const [error, setError] = useState(recovery.error);
  const [busy, setBusy] = useState(false);
  const [discard, setDiscard] = useState(false);
  const [removing, setRemoving] = useState<string>();
  const [visibleCount, setVisibleCount] = useState(10);
  const inFlight = useRef(false);
  const dirty = !saved && Boolean(contactEdited.current.email || contactEdited.current.name || subject || body || review || files.length);
  function close() { if (dirty && !attempted) setDiscard(true); else onClose(); }
  const modal = useModalFocus<HTMLElement>(close);

  useEffect(() => {
    // Read only: opening feedback must not publish identity or alter a retained
    // report. A slow response must not overwrite contact details already edited.
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 5_000);
    void fetchPublicHiveProfile(operatorToken, controller.signal).then(({ profile }) => {
      if (controller.signal.aborted) return;
      const savedName = profile.operator_display_name === "Operator" ? "" : profile.operator_display_name;
      if (!contactEdited.current.name) setName(savedName);
      if (!contactEdited.current.email) setEmail(profile.contact_email ?? "");
      setSavedContact(Boolean(savedName || profile.contact_email));
    }).catch(() => { /* Unavailable identity never blocks writing a support message. */ })
      .finally(() => window.clearTimeout(deadline));
    return () => { controller.abort(); window.clearTimeout(deadline); };
  }, [operatorToken]);

  useEffect(() => {
    if (typeof indexedDB === "undefined") return;
    let active = true;
    void loadPendingSupportFiles().then((pending) => {
      if (!active || !pending) return;
      if (recovery.pending) throw new Error("A text report and an attachment report both await confirmation. Check delivery status before sending another.");
      setReview(pending.submission); setFiles(pending.files); setAttempted(true);
    }).catch((failure: unknown) => {
      if (active) { setError(failure instanceof Error ? failure.message : "Saved files could not be recovered."); setFilesRecoveryFailed(true); }
    }).finally(() => { if (active) setRecoveringFiles(false); });
    return () => { active = false; };
  }, [recovery.pending]);

  async function selectFiles(selected: File[]) {
    if (inFlight.current || !selected.length) return;
    inFlight.current = true; setBusy(true); setReadingFiles(true); setError("");
    try { setFiles(await prepareSupportFiles(selected)); }
    catch (failure) { setError(failure instanceof Error ? failure.message : "The files could not be read. No report was sent."); }
    finally { inFlight.current = false; setBusy(false); setReadingFiles(false); }
  }

  async function clearBrowserCopy(key: string) {
    if (files.length) await clearPendingSupportFiles(key);
    else clearPendingSupport();
  }

  async function send() {
    if (!review || inFlight.current) return;
    inFlight.current = true; setBusy(true); setError("");
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), files.length ? 65_000 : 15_000);
    let retained = false;
    try {
      // If browser persistence fails, do not risk an unrecoverable duplicate.
      if (files.length) await savePendingSupportFiles({ submission: review, files });
      else savePendingSupport(review);
      retained = true;
      setAttempted(true);
      const result = files.length
        ? await submitSupportFiles(operatorToken, { submission: review, files }, controller.signal)
        : await submitSupport(operatorToken, review, controller.signal);
      setSaved(result);
      setStatus((old) => ({ ...old, deliveries: [result, ...old.deliveries.filter((row) => row.submission_key !== result.submission_key)] }));
      try { await clearBrowserCopy(result.submission_key); } catch { /* exact replay remains safe if browser cleanup fails */ }
      onSaved?.();
    } catch (failure) {
      setError(!retained && failure instanceof Error ? failure.message : "The save could not be confirmed. Keep this exact report and retry; a retry will not create a second report.");
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
      if (known && saved?.submission_key === known.submission_key) {
        setSaved(known); try { await clearBrowserCopy(known.submission_key); } catch { /* retained exact key is safe */ }
      } else if (known) {
        // Status contains identity, not a reviewed-content digest. Only exact POST
        // replay can establish that the saved report includes these same files/text.
        setError("A report with this key is saved. Retry this exact report to confirm its contents before removing the browser copy.");
      }
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

  return <ModalPortal><div className="feedback-backdrop" role="presentation">
    <section ref={modal} tabIndex={-1} className="feedback-dialog support-feedback-dialog" role="dialog" aria-modal="true" aria-labelledby="support-heading">
      <header><div><p className="eyebrow">A note to the hive keepers</p><h2 id="support-heading">Swarm Support</h2></div>
        <button type="button" className="secondary-button" onClick={close}>Close</button></header>
      <p>Share feedback privately with Swarm Support. Only an email address is required for contact. No GitHub account is needed.</p>
      {!status.configured && <p role="status">Central support is disabled. Existing reports are retained; new reports cannot be sent here.</p>}
      {recoveringFiles && <p role="status">Checking for a saved attachment report…</p>}
      {!review && !recovery.error && !recoveringFiles && !filesRecoveryFailed && status.configured ? <form onSubmit={(event) => { event.preventDefault(); setReview({
        submission_key: crypto.randomUUID(), kind, email, name: name || null, subject, body,
      }); }}>
        <div className="feedback-fields">
          <label>Email<input type="email" required autoComplete="email" maxLength={320} value={email} onChange={(event) => { contactEdited.current.email = true; setEmail(event.target.value); }} /></label>
          <label>Name (optional)<input autoComplete="name" maxLength={200} value={name} onChange={(event) => { contactEdited.current.name = true; setName(event.target.value); }} /></label>
          {savedContact && <small>Filled from your saved Hive profile. Changes here apply only to this message.</small>}
          <label>Type<select value={kind} onChange={(event) => setKind(event.target.value as SupportSubmission["kind"])}>
            <option value="bug_report">Bug report</option><option value="feature_request">Feature request</option><option value="feedback">Feedback</option>
          </select></label>
          <label>Subject<input required maxLength={240} value={subject} onChange={(event) => setSubject(event.target.value)} /></label>
          <label className="support-message-field">Message<textarea required maxLength={20000} value={body} onChange={(event) => setBody(event.target.value)} /></label>
          {status.attachments_supported && typeof indexedDB !== "undefined" && <label className="support-message-field">Attachments (optional)
            <input type="file" accept="image/png,image/jpeg,image/webp,text/plain" multiple disabled={busy} onChange={(event) => { const selected = [...(event.target.files ?? [])]; event.target.value = ""; void selectFiles(selected); }} />
            <small>Up to 4 PNG, JPEG, WebP or text files. 5 MiB each, 12 MiB total. Selecting again replaces the list.</small>
          </label>}
        </div>
        {readingFiles && <p role="status">Reading selected files…</p>}
        {!!files.length && <ul className="support-files">{files.map((file) => <li key={file.metadata.id}><span>{file.metadata.file_name} · {Math.ceil(file.bytes.byteLength / 1024)} KiB</span>
          <button type="button" className="secondary-button" disabled={busy} onClick={() => setFiles((old) => old.filter((item) => item.metadata.id !== file.metadata.id))}>Remove {file.metadata.file_name}</button></li>)}</ul>}
        <button className="primary-action" type="submit" disabled={busy || !email.trim() || !subject.trim() || !body.trim()}>Review message</button>
      </form> : review ? <section aria-label="Review support message">
        <h3>{review.subject}</h3><p>{review.name ? `${review.name} · ` : ""}{review.email}</p>
        <pre className="support-review-text">{review.body}</pre>
        {!!files.length && <div aria-label="Reviewed attachments"><h4>Files included</h4><ul className="support-files">{files.map((file) => <li key={file.metadata.id}>
          <span>{file.metadata.file_name} · {Math.ceil(file.bytes.byteLength / 1024)} KiB</span></li>)}</ul>
          <p>The exact selected files will be shared, including any image metadata. No files are added automatically.</p></div>}
        {saved ? <p role="status">{deliveryLabel(saved)}</p> : <div className="diagnostic-actions">
          {!attempted && <button type="button" className="secondary-button" onClick={() => setReview(undefined)}>Edit message</button>}
          <button type="button" className="primary-action" disabled={busy || !status.configured || (!!files.length && !status.attachments_supported)} onClick={() => void send()}>
            {busy ? "Saving…" : attempted ? "Retry this exact report" : "Send to Swarm Support"}</button>
        </div>}
      </section> : null}
      {!!files.length && !status.attachments_supported && <p role="status">This Hive does not currently support file uploads. The original report is retained; update the Hive before retrying.</p>}
      <small className="privacy-note">Only the reviewed message, contact details and selected files are sent. Swarm Support may reply by email. Diagnostics are never uploaded automatically.</small>
      {!saved && <small className="privacy-note">{attempted
        ? "A retry copy stays in this browser until the Hive confirms saving it. Reopening never sends it automatically."
        : "Your draft has not been saved or sent. Closing asks before discarding it; reloading this page can lose it."}</small>}
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
  </div></ModalPortal>;
}
