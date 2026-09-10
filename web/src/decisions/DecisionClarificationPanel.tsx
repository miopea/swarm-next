import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type { DecisionClarification } from "../api";
import LongText from "./LongText";

type Props = {
  requester: string;
  pending: boolean;
  waiting: boolean;
  history: DecisionClarification[];
  roundCount?: number;
  workerNames: ReadonlyMap<string, string>;
  loading?: boolean;
  loadError?: string;
  onReload: () => void;
  onAsk: (id: string, question: string) => Promise<DecisionClarification>;
};

/** No answer/resolve callback: an explanation cannot accidentally become approval. */
export default function DecisionClarificationPanel({
  requester, pending, waiting, history, roundCount = history.length, workerNames, loading = false, loadError, onReload, onAsk,
}: Props) {
  const labelId = useId();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string>();
  const [sentId, setSentId] = useState<string>();
  const attempt = useRef<{ id: string; question: string } | undefined>(undefined);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const askButton = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  const region = useRef<HTMLElement>(null);
  const receiptStatus = useRef<HTMLParagraphElement>(null);
  const focusReceipt = useRef(false);
  const inFlight = useRef(false);
  const bytes = new TextEncoder().encode(draft).length;
  const receiptPending = Boolean(sentId && !history.some((round) => round.id === sentId));
  const latest = history.at(-1);
  const awaitingReply = waiting || receiptPending || Boolean(latest && latest.reply === null && latest.delivery_state !== "cancelled");
  const latestReplyName = latest?.replying_worker_id
    ? workerNames.get(latest.replying_worker_id) ?? "Worker" : requester;

  useEffect(() => {
    if (editing) textarea.current?.focus();
    else if (restoreFocus.current && askButton.current) {
      askButton.current.focus();
      restoreFocus.current = false;
    }
  }, [editing]);

  useEffect(() => {
    if (sentId && focusReceipt.current) receiptStatus.current?.focus();
    focusReceipt.current = false;
  }, [sentId]);

  async function send(event: FormEvent) {
    event.preventDefault();
    if (!pending || awaitingReply || inFlight.current || !draft.trim() || bytes > 4000) return;
    if (!attempt.current || attempt.current.question !== draft) {
      attempt.current = { id: crypto.randomUUID(), question: draft };
    }
    const current = attempt.current;
    inFlight.current = true;
    setSending(true);
    setError(undefined);
    try {
      const saved = await onAsk(current.id, current.question);
      // Keep the keyboard at the result of this submission, but never pull it
      // back from another decision the operator focused while the send waited.
      focusReceipt.current = Boolean(region.current?.contains(document.activeElement)) || document.activeElement === document.body;
      setSentId(saved.id);
      setDraft("");
      setEditing(false);
      attempt.current = undefined;
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Your question could not be sent. Your draft is still here.");
    } finally {
      inFlight.current = false;
      setSending(false);
    }
  }

  return <section ref={region} className="decision-clarification" aria-label="Questions about this decision">
    {pending && awaitingReply && <p ref={receiptStatus} role="status" tabIndex={-1}>Waiting for {requester} to reply. You can still make your decision below.</p>}
    {latest?.reply && <div className="clarification-reply">
      <p className="eyebrow">{latestReplyName} replied{pending ? "" : " · saved in history"}</p>
      <LongText text={latest.reply} label="the explanation" foldAbove={450} />
    </div>}
    {pending && !awaitingReply && !editing && <button ref={askButton} type="button" className="text-button" onClick={() => setEditing(true)}>Ask a question</button>}
    {pending && editing && <form onSubmit={(event) => { void send(event); }}>
      <label htmlFor={labelId}>What would you like clarified?</label>
      <p id={`${labelId}-hint`} className="muted">Ask without deciding. Your original choices stay available.</p>
      <textarea ref={textarea} id={labelId} rows={3} value={draft} disabled={sending}
        aria-describedby={`${labelId}-hint${bytes > 4000 ? ` ${labelId}-limit` : ""}`}
        aria-invalid={bytes > 4000 || undefined}
        onChange={(event) => { setDraft(event.target.value); setError(undefined); }} />
      {bytes > 4000 && <p id={`${labelId}-limit`} role="alert">Please shorten your question (4,000-byte limit).</p>}
      {error && <p role="alert">{error}</p>}
      <div className="decision-actions">
        <button type="submit" disabled={sending || awaitingReply || !draft.trim() || bytes > 4000}>{sending ? "Sending question…" : "Send question"}</button>
        <button type="button" className="text-button" disabled={sending} onClick={() => { restoreFocus.current = true; setEditing(false); }}>Keep draft for later</button>
      </div>
    </form>}
    {(roundCount > 0 || awaitingReply || loading || loadError) && <details onToggle={(event) => { if (event.currentTarget.open && !loading) onReload(); }}>
      <summary>Questions &amp; replies{roundCount ? ` (${roundCount})` : ""}</summary>
      {loading && <p role="status">Loading the conversation…</p>}
      {loadError && <div role="alert"><p>{loadError}</p><button type="button" onClick={onReload}>Try again</button></div>}
      {!loading && !loadError && !history.length && <p>No questions yet.</p>}
      <ol>{history.map((round) => <li key={round.id}>
        <p className="eyebrow">You asked</p>
        <LongText text={round.question} label="your question" foldAbove={450} />
        {round.reply ? <><p className="eyebrow">{workerNames.get(round.replying_worker_id ?? "") ?? "Worker"} replied</p><LongText text={round.reply} label="this reply" foldAbove={450} /></>
          : <p className="muted">{round.delivery_state === "cancelled" ? "No reply needed." : round.delivery_state === "uncertain" ? "Delivery is unconfirmed. Check the worker before sending again." : "Waiting for a reply."}</p>}
      </li>)}</ol>
    </details>}
  </section>;
}
