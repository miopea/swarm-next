import { useState } from "react";
import { setWorkerEngineAutoUpdate, type WorkerEngineAutoUpdateView } from "../api";
import { engineAutoUpdateStatus, type WorkingWorker } from "../runtime/engineAutoUpdate";

type Props = {
  operatorToken?: string;
  view?: WorkerEngineAutoUpdateView | null;
  /** Whether an engine update is waiting now. */
  behind: boolean;
  /** Who the roster shows busy now, which holds the update too. */
  working?: WorkingWorker[];
  /** The last update's history line, when this card is where it is shown. */
  history?: string | null;
};

/**
 * Whether a waiting worker engine update applies by itself (ADR 0156), shown
 * on the worker engine card with the switch that turns it off.
 *
 * The operator ruled "keep it automatic, but say so when it happens". So the
 * card says it will happen, what it is waiting for, and what it did last.
 */
export default function WorkerEngineAutoUpdate({ operatorToken, view, behind, working, history }: Props) {
  const [chosen, setChosen] = useState<boolean>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  if (!view) return history ? <p className="runtime-engine-history" role="status">{history}</p> : null;
  const enabled = chosen ?? view.enabled;

  async function choose(next: boolean) {
    if (!operatorToken) return;
    setSaving(true);
    setError(undefined);
    try {
      setChosen((await setWorkerEngineAutoUpdate(operatorToken, next)).enabled);
    } catch {
      setError("The setting was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  const status = behind ? engineAutoUpdateStatus({ ...view, enabled }, working) : null;
  return (
    <div className="engine-auto-update">
      <label className="worker-autostart">
        <input
          type="checkbox"
          checked={enabled}
          disabled={!operatorToken || saving}
          onChange={(event) => void choose(event.target.checked)}
        />
        <span>
          <strong>Update by itself when every worker is resting</strong>
          <small>Swarm stops only resting Claude workers, brings them back, and says so here. Off, an engine update waits for the button.</small>
        </span>
      </label>
      {status ? (
        <div className="engine-auto-update-status" role="status">
          <p>{status.summary}</p>
          {status.holders.length > 0 ? (
            <ul>{status.holders.map((holder) => <li key={holder}>{holder}</li>)}</ul>
          ) : null}
          {status.notes.map((note) => <p key={note}>{note}</p>)}
        </div>
      ) : null}
      {history ? <p className="runtime-engine-history" role="status">{history}</p> : null}
      {error ? <p role="alert">{error}</p> : null}
    </div>
  );
}
