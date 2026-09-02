import { useState } from "react";

/**
 * One thing said to every running worker.
 *
 * The operator asked for this on 2026-09-02, having been pausing workers one
 * terminal at a time. It is a MESSAGE, not a stop: delivery defers while a
 * worker is mid-turn and arrives when its terminal is resting, so a broadcast
 * cannot take somebody's thread with it.
 *
 * IT REPORTS WHO IT DID NOT REACH, and that is the part worth the component.
 * A worker with no live session is excluded from delivery rather than queued
 * for it — 13 of 45 had one when this was built — so a broadcast that answered
 * "sent" would let the operator believe everyone was told. Being told "13 of
 * 45" is what makes it safe to rely on.
 */
export default function BroadcastToWorkers({
  onBroadcast,
}: {
  onBroadcast: (body: string) => Promise<{ reached: number; skipped: number }>;
}) {
  const [open, setOpen] = useState(false);
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ reached: number; skipped: number }>();
  const [error, setError] = useState<string>();

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

  if (!open) {
    return (
      <button type="button" className="broadcast-open" onClick={() => setOpen(true)}>
        Tell every worker
      </button>
    );
  }

  return (
    <div className="broadcast" role="group" aria-label="Broadcast to every worker">
      <label className="sr-only" htmlFor="broadcast-body">What to tell every running worker</label>
      <textarea
        id="broadcast-body"
        rows={3}
        maxLength={4000}
        placeholder="Reloading the engine in five minutes — please park what you are doing."
        value={body}
        onChange={(event) => setBody(event.target.value)}
      />
      <div className="broadcast-actions">
        <button type="button" onClick={() => void send()} disabled={busy || !body.trim()}>
          {busy ? "Sending…" : "Send to every worker"}
        </button>
        <button type="button" className="ghost" onClick={() => { setOpen(false); setResult(undefined); setError(undefined); }}>
          Close
        </button>
      </div>
      {/* Never "sent". The count is the whole point: a worker with no live
          session is skipped, not queued, and saying so is what stops the
          operator believing everyone was told. */}
      {result ? (
        <p className="broadcast-result" role="status">
          Queued for {result.reached} running worker{result.reached === 1 ? "" : "s"}
          {result.skipped > 0 ? ` · ${result.skipped} had no live session and were not reached` : ""}.
          {result.reached > 0 ? " Each arrives when that terminal is resting." : ""}
        </p>
      ) : null}
      {error ? <p className="broadcast-error" role="alert">{error}</p> : null}
    </div>
  );
}
