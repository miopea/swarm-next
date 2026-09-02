import { useEffect, useRef, useState } from "react";

/**
 * One thing said to every running worker, from a modal.
 *
 * The operator asked for this on 2026-09-02, having been pausing workers one
 * terminal at a time. It is a MESSAGE, not a stop: delivery defers while a
 * worker is mid-turn and arrives when its terminal is resting, so a broadcast
 * cannot take somebody's thread with it.
 *
 * A MODAL BEHIND AN ICON, not a panel in the rail. The first version put a
 * full-width primary button above the worker search; the operator's words were
 * "This is a waste of the UI" — and they were right. Broadcasting is rare and
 * deliberate, so it earns a header icon like Report a problem and quick
 * navigation, not permanent vertical space in a list people scan constantly.
 *
 * IT REPORTS WHO IT DID NOT REACH, and that is the part worth the component.
 * A worker with no live session is excluded from delivery rather than queued
 * for it — 13 of 45 had one when this was built — so a broadcast that answered
 * "sent" would let the operator believe everyone was told.
 */
export default function BroadcastToWorkers({
  open,
  onClose,
  onBroadcast,
}: {
  open: boolean;
  onClose: () => void;
  onBroadcast: (body: string) => Promise<{ reached: number; skipped: number }>;
}) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ reached: number; skipped: number }>();
  const [error, setError] = useState<string>();
  const field = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (open) field.current?.focus();
  }, [open]);

  if (!open) return null;

  async function send() {
    if (!body.trim() || busy) return;
    setBusy(true);
    setError(undefined);
    try {
      const outcome = await onBroadcast(body.trim());
      setResult(outcome);
      setBody("");
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The broadcast could not be sent.");
    } finally {
      setBusy(false);
    }
  }

  function close() {
    setResult(undefined);
    setError(undefined);
    setBody("");
    onClose();
  }

  return (
    <div className="modal-backdrop" role="presentation" onClick={close}>
      <div
        className="modal broadcast-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Tell every worker"
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => { if (event.key === "Escape") close(); }}
      >
        <h2>Tell every worker</h2>
        <p className="broadcast-explainer">
          Goes to every worker with a live session. It waits until each terminal is resting, so it
          will not interrupt work in progress.
        </p>
        <label className="sr-only" htmlFor="broadcast-body">What to tell every running worker</label>
        <textarea
          id="broadcast-body"
          ref={field}
          rows={4}
          maxLength={4000}
          placeholder="Reloading the engine shortly — please park what you are doing."
          value={body}
          onChange={(event) => setBody(event.target.value)}
        />
        {/* Never "sent". The count is the whole point: a worker with no live
            session is skipped, not queued, and saying so is what stops the
            operator believing everyone was told. */}
        {result ? (
          <p className="broadcast-result" role="status">
            Queued for {result.reached} running worker{result.reached === 1 ? "" : "s"}
            {result.skipped > 0 ? ` · ${result.skipped} had no live session and were not reached` : ""}.
          </p>
        ) : null}
        {error ? <p className="broadcast-error" role="alert">{error}</p> : null}
        <div className="settings-actions">
          <button type="button" className="secondary-button" onClick={close}>Close</button>
          <button type="button" className="primary-action" onClick={() => void send()} disabled={busy || !body.trim()}>
            {busy ? "Sending…" : "Send to every worker"}
          </button>
        </div>
      </div>
    </div>
  );
}
