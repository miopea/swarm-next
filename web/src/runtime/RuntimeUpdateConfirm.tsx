import type { RuntimeUpdateSummary } from "./runtimeUpdates";

/**
 * Asks before running a runtime update from the control room.
 *
 * The operator asked to start these without opening Settings, and said a
 * confirmation is fine — with a stronger warning for the ones that take workers
 * away. So the weight of this dialog is set by `consequence`: an App and API
 * release keeps every worker online and reads as a plain question, while a
 * worker engine or provider restart stops running work and says exactly what
 * stops before offering the button.
 */
export default function RuntimeUpdateConfirm({ update, busy, onConfirm, onCancel }: {
  update: RuntimeUpdateSummary;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const destructive = Boolean(update.consequence);
  return (
    <div className="dialog-backdrop" role="presentation" onClick={onCancel}>
      <div
        className={`dialog runtime-confirm${destructive ? " destructive" : ""}`}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="runtime-confirm-heading"
        aria-describedby="runtime-confirm-detail"
        onClick={(event) => event.stopPropagation()}
      >
        <p className="eyebrow">{destructive ? "This stops running work" : "Runtime update"}</p>
        <h3 id="runtime-confirm-heading">{update.actionLabel}</h3>
        <p id="runtime-confirm-detail">{update.detail}</p>
        {update.consequence ? (
          <p className="runtime-confirm-consequence" role="alert">{update.consequence}</p>
        ) : null}
        <div className="dialog-actions">
          <button type="button" className="secondary-button" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className={destructive ? "destructive-action" : "primary-action"}
            onClick={onConfirm}
            disabled={busy}
            autoFocus
          >
            {busy ? "Working…" : update.actionLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
