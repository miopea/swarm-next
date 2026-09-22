import { useState } from "react";

import type { TakeoverLease } from "./api";

type Props = {
  leases: TakeoverLease[] | undefined;
  onReclaim: (leaseId: string, reason: string) => Promise<void> | void;
};

/**
 * Says, on every surface, that somebody else is controlling this Hive — and
 * hands it back from here.
 *
 * ⚠️ THIS COMPONENT IS A RELEASE CONDITION, NOT A COURTESY. ADR 0036 refuses to
 * ship takeover at all unless activation visibly replaces the local engagement
 * lease and the operator can reclaim from any authenticated local surface. If
 * this stops rendering, the capability becomes remote control of someone's
 * machine without disclosure, and it must not be available in that state.
 *
 * ⚠️ IT IS STYLED AS AN ALARM, LOUDER THAN THE WATCH NOTICE, because the two
 * are different things. Someone watching is reading your screen; someone in
 * takeover is typing on your machine. A single visual treatment for both would
 * teach the operator to skim past the one that matters.
 */
export default function TakeoverNotice({ leases, onReclaim }: Props) {
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  // Defensive for the same reason the watch notice is: this failing to render
  // is not a cosmetic bug, it is the forbidden state.
  const held = (Array.isArray(leases) ? leases : []).filter(
    (lease) => Boolean(lease?.id) && (lease.state === "requested" || lease.state === "active"),
  );
  if (held.length === 0) return null;
  const lease = held[0];
  const active = lease.state === "active";

  return (
    <div className="takeover-notice" role="alert">
      <strong>{active ? "Someone else is controlling this Hive" : "Someone is asking to control this Hive"}</strong>
      <small>{lease.reason}</small>
      {/* A reason is required, and it is the operator's own account — the audit
          can answer "why was this taken back" only because this field exists. */}
      <label>
        Why are you taking it back?
        <input
          type="text"
          value={reason}
          onChange={(event) => setReason(event.target.value)}
          placeholder="Mid-deploy; taking my machine back"
        />
      </label>
      <button
        type="button"
        disabled={busy || reason.trim().length === 0}
        onClick={async () => {
          setBusy(true);
          try {
            await onReclaim(lease.id, reason.trim());
            setReason("");
          } finally {
            setBusy(false);
          }
        }}
      >
        Take back control
      </button>
    </div>
  );
}
