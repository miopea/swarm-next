import type { ApiaryUsage, HiveUsage } from "../api";
import { busyTime, compact, shortWorkspace } from "../settings/usageFormat";

type Props = {
  /** Read with the rest of the Keeper page, so it recovers when the page does. */
  usage: ApiaryUsage | undefined;
  days: number;
  onDays: (days: number) => void;
  nameFor: (hiveId: string) => string | undefined;
};

export const USAGE_WINDOWS = [7, 30] as const;
/** The most workspaces named under one Hive; the rest are summed. */
const SHOWN_WORKSPACES = 5;

/**
 * What every Hive in the Apiary spent (ADR 0114 addendum): provider tokens
 * and worker-busy time, by Hive and by model, with the Apiary's totals.
 *
 * The operator asked for it "time or token based or both" — both. Each Hive's
 * figures say how old they are, because a member reports them every few
 * minutes and a stale row reads exactly like a quiet one.
 */
export default function ApiaryUsagePanel({ usage, days, onDays, nameFor }: Props) {
  const hives = [...(usage?.hives ?? [])].sort((left, right) => right.totals.weighted - left.totals.weighted || right.totals.active_seconds - left.totals.active_seconds);
  const total = usage?.totals.weighted ?? 0;
  const name = (hive: HiveUsage) => `${nameFor(hive.hive_id) ?? hive.hive_id}${hive.is_local ? " (this Hive)" : ""}`;

  return (
    <article className="keeper-panel apiary-usage-panel" aria-labelledby="apiary-usage-heading">
      <header>
        <div><p className="eyebrow">Usage</p><h4 id="apiary-usage-heading">What the Apiary spent</h4></div>
        <div className="usage-window" role="group" aria-label="Window">
          {USAGE_WINDOWS.map((option) => (
            <button key={option} type="button" aria-pressed={days === option} onClick={() => onDays(option)}>{option} days</button>
          ))}
        </div>
      </header>
      <p className="apiary-policy-note">
        Weighted tokens put input, output and cache on one scale by model; a comparison, never a bill. Busy time is worker time from terminal activity, and approximate.
      </p>
      {usage === undefined ? <p className="keeper-empty">Usage unavailable. It is read again when you come back to this page.</p> : (
        <>
          <table className="apiary-usage-table">
            <caption className="visually-hidden">Spend by Hive over {usage.days} days</caption>
            <thead><tr><th scope="col">Hive</th><th scope="col">Weighted</th><th scope="col">Busy</th></tr></thead>
            <tbody>
              {hives.map((hive) => (
                <tr key={hive.hive_id}>
                  <th scope="row">
                    <details>
                      <summary>{name(hive)}</summary>
                      <WorkspaceList hive={hive} />
                    </details>
                    {unreported(hive) ? <small>No figures yet: its release may be too old to send them.</small> : null}
                  </th>
                  {/* No figures is not zero spend. */}
                  <td>{unreported(hive) ? "—" : compact(hive.totals.weighted)}</td>
                  <td>{unreported(hive) ? "—" : busyTime(hive.totals.active_seconds)}</td>
                </tr>
              ))}
            </tbody>
            <tfoot><tr><th scope="row">Apiary</th><td>{compact(total)}</td><td>{busyTime(usage.totals.active_seconds)}</td></tr></tfoot>
          </table>
          {usage.by_model.length ? (
            <ul className="apiary-usage-models" aria-label="Spend by model">
              {usage.by_model.map((model) => (
                <li key={model.name}><span>{model.name}</span><strong>{compact(model.weighted)}</strong><small>{total > 0 ? `${Math.round((model.weighted / total) * 100)}%` : "—"}</small></li>
              ))}
            </ul>
          ) : <p className="keeper-empty">No provider usage in this window.</p>}
        </>
      )}
    </article>
  );
}

/** A member that has never sent its figures, which is not the same as spending nothing. */
function unreported(hive: HiveUsage): boolean {
  return hive.received_at === null && !hive.is_local;
}

function WorkspaceList({ hive }: { hive: HiveUsage }) {
  if (!hive.by_workspace.length) return <p className="keeper-empty">Nothing in this window.</p>;
  const shown = hive.by_workspace.slice(0, SHOWN_WORKSPACES);
  const rest = hive.by_workspace.slice(SHOWN_WORKSPACES);
  return (
    <ul className="apiary-usage-workspaces">
      {shown.map((workspace) => (
        <li key={workspace.name} title={workspace.name}><span>{shortWorkspace(workspace.name)}</span><span>{compact(workspace.weighted)} · {busyTime(workspace.active_seconds)}</span></li>
      ))}
      {rest.length ? <li><span>{rest.length} more</span><span>{compact(rest.reduce((sum, workspace) => sum + workspace.weighted, 0))}</span></li> : null}
    </ul>
  );
}
