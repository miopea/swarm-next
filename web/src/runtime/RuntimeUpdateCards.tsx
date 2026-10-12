import type { RuntimeUpdateSummary } from "./runtimeUpdates";

type Props = {
  updates: RuntimeUpdateSummary[];
  /** Another runtime action is in flight, so none can be started. */
  busy: boolean;
  /** Asks to run one; the caller confirms what it costs first. */
  onRun: (update: RuntimeUpdateSummary) => void;
  /** Opens Settings → Updates, where each has its full card. */
  onDetails: () => void;
};

/**
 * The rail's runtime updates: one per subsystem rather than only the most
 * severe, because they are independent and can all be true at once, and
 * ranking them into a single pill hid the others until the first was dealt
 * with.
 *
 * ⚠️ A PENDING UPDATE IS ONE LINE. The operator often defers one on purpose
 * for hours, until they are ready to restart workers or the API. As full cards
 * two of them took 400px of the rail and left the worker list at nothing
 * (2026-10-12). A deferred update is a known state, not an alarm, so it is not
 * red either. It keeps three things:
 * - its name, which opens the full explanation in Settings;
 * - its action, which still confirms what it costs;
 * - that cost in a few words.
 * Work in progress and failures keep the full card, because they are the ones
 * with something new to say.
 */
export default function RuntimeUpdateCards({ updates, busy, onRun, onDetails }: Props) {
  return (
    <>
      {updates.map((update) => !update.busy && update.kind !== "failed" ? (
        <div key={update.kind} className={`runtime-update-card compact runtime-update-${update.kind}`}>
          <div className="runtime-update-row">
            <button type="button" className="runtime-update-name" onClick={onDetails}>{update.label}</button>
            {update.action ? (
              // The visible word starts the accessible name, which says it all.
              <button type="button" className="runtime-update-run" aria-label={update.actionLabel} onClick={() => onRun(update)} disabled={busy}>
                {update.actionLabel?.split(" ")[0]}
              </button>
            ) : null}
          </div>
          {update.costShort ? <p className="runtime-update-cost">{update.costShort}</p> : null}
        </div>
      ) : (
        <div key={update.kind} className={`runtime-update-card runtime-update-${update.kind}${update.consequence ? " costly" : ""}`}>
          <p className="runtime-update-label">
            {update.busy ? <span className="runtime-update-spinner" aria-hidden="true" /> : null}
            {update.label}
          </p>
          <p className="runtime-update-detail">{update.detail}</p>
          {update.action && !update.busy ? (
            <button type="button" className="runtime-update-run" onClick={() => onRun(update)} disabled={busy}>{update.actionLabel}</button>
          ) : null}
          <button type="button" className="runtime-update-settings" onClick={onDetails}>Details</button>
        </div>
      ))}
    </>
  );
}
