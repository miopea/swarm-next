import type { Worker } from "../api";
import { WorkerCapabilities } from "./WorkerSettings";

function holdsRaisedAccess(worker: Worker): boolean {
  return worker.board_read || worker.system_access !== "none";
}

/**
 * Which workers may do more than their own assignment, beside the other rules
 * about what happens without the operator.
 *
 * Read-only on purpose. Each worker's access is set in its own editor on the
 * roster, and a second place to change it would be a second place to miss. This
 * is the audit: the roster marks the few rows that hold anything, but only
 * among every other row, below the add-worker form.
 *
 * Commands granted to a worker on another provider are listed and marked as
 * not in effect. Only a Claude launch writes them (ADR 0132), and hiding the
 * setting would hide what that worker gains the day it moves to Claude.
 */
export default function WorkerAccessSettings({ workers, onShowWorker }: {
  workers: readonly Worker[];
  onShowWorker: (workerId: string) => void;
}) {
  const raised = workers.filter(holdsRaisedAccess);
  return (
    <section id="settings-worker-access" className="settings-card worker-access-settings" aria-labelledby="worker-access-heading">
      <div><p className="eyebrow">Worker access</p><h3 id="worker-access-heading">Workers that may do more than their own task</h3></div>
      {raised.length === 0 ? (
        <p>No worker can read the whole board or run machine commands without asking you.</p>
      ) : (
        <ul className="worker-access-list">
          {raised.map((worker) => (
            <li key={worker.id}>
              <span>
                <strong>{worker.name}</strong>
                <WorkerCapabilities worker={worker} />
                {worker.system_access !== "none" && worker.provider !== "claude_code"
                  ? <small>Its commands are not in effect: only Claude workers use them.</small>
                  : null}
              </span>
              <button type="button" className="secondary-button" aria-label={`Edit ${worker.name}`} onClick={() => onShowWorker(worker.id)}>Edit</button>
            </li>
          ))}
        </ul>
      )}
      <small>Set in each worker&apos;s own settings under Workers. A change to its commands reaches a running worker the next time it starts.</small>
    </section>
  );
}
