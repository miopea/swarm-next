import type { RuntimeUpdateSummary } from "./runtimeUpdates";
import { useRef } from "react";
import { useModalFocus } from "../shared/useModalFocus";

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
  const confirm = useRef<HTMLButtonElement>(null);
  const cancel = () => { if (!busy) onCancel(); };
  const dialog = useModalFocus<HTMLDivElement>(cancel, true, destructive ? undefined : confirm);
  return (
    <div className="dialog-backdrop" role="presentation" onClick={cancel}>
      <div
        ref={dialog}
        tabIndex={-1}
        className={`dialog runtime-confirm${destructive ? " destructive" : ""}`}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="runtime-confirm-heading"
        aria-describedby={destructive ? "runtime-confirm-detail runtime-confirm-consequence" : "runtime-confirm-detail"}
        onClick={(event) => event.stopPropagation()}
      >
        <p className="eyebrow">{destructive ? "This stops running work" : "Runtime update"}</p>
        <h3 id="runtime-confirm-heading">{update.actionLabel}</h3>
        <p id="runtime-confirm-detail">{update.detail}</p>
        {update.consequence ? (
          <p id="runtime-confirm-consequence" className="runtime-confirm-consequence" role="alert">{update.consequence}</p>
        ) : null}
        <div className="dialog-actions">
          <button type="button" className="secondary-button" onClick={cancel} disabled={busy}>
            Cancel
          </button>
          <button
            ref={confirm}
            type="button"
            className={destructive ? "destructive-action" : "primary-action"}
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? "Working…" : update.actionLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
