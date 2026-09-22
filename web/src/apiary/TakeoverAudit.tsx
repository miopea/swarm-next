import type { TakeoverAuditEntry } from "../api";

/**
 * Who took over which Hive, when, why, and why it ended.
 *
 * ⚠️ THIS IS PART OF ADR 0036'S RELEASE GATE, NOT A REPORT. Takeover is the one
 * Apiary capability that lets someone type into another operator's machine. An
 * audit that existed only in the database would make that accountable in
 * principle and unreviewable in practice, which the ADR treats as the same
 * thing as not having one.
 *
 * It shows both reasons, because they are different claims: why a takeover
 * started, and the local operator's account of taking their machine back. The
 * second is the one that matters when the question is whether the first should
 * have happened.
 */
export default function TakeoverAudit({ entries, nameFor }: {
  entries?: TakeoverAuditEntry[];
  nameFor?: (hiveId: string) => string | undefined;
}) {
  const rows = Array.isArray(entries) ? entries : [];
  return <article className="keeper-panel">
    <header>
      <div><p className="eyebrow">Accountability</p><h4>Takeovers</h4></div>
      <small>Who held a Hive, never what they did while holding it</small>
    </header>
    {rows.length === 0
      ? <p className="keeper-empty">No Hive has been taken over.</p>
      : <ul className="keeper-work-list" aria-label="Takeover history">{rows.map((entry) => <li key={entry.lease.id}>
        <span>
          <strong>{nameFor?.(entry.lease.target_hive_id) ?? entry.lease.target_hive_id}</strong>
          <small>
            {entry.lease.stewardship_id === null ? "Keeper" : "Steward"} · {entry.lease.reason}
          </small>
        </span>
        <span>
          <strong>{stateLabel[entry.lease.state]}</strong>
          {/* The operator's own words, shown as theirs rather than folded into
              a status. A reclaim is someone saying "not now, this is mine". */}
          <small>{entry.reclaim_reason ? `Reclaimed: ${entry.reclaim_reason}` : `Revision ${entry.lease.revision}`}</small>
        </span>
      </li>)}</ul>}
  </article>;
}

const stateLabel: Record<TakeoverAuditEntry["lease"]["state"], string> = {
  requested: "Requested",
  active: "Active now",
  released: "Released",
  reclaimed: "Reclaimed by operator",
  expired: "Expired",
};
