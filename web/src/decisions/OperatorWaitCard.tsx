import type { OperatorWait } from "../api";

type Props = {
  waits: OperatorWait[];
  busy?: boolean;
  onDone: (taskId: string) => void;
  onOpenTask?: (taskId: string) => void;
};

/**
 * Work that cannot move until the operator does something (ADR 0119).
 *
 * ⚠️ THIS IS WHY THE BADGE READ 0 WHILE FOUR TASKS WAITED ON THEM. A worker
 * that needed the operator to sign in somewhere had no way to say so except in
 * prose, so the work sat Active or Blocked and nothing reached this page. Each
 * item here is one thing the operator alone can do, and Done sends the task
 * straight back to its worker.
 *
 * ⚠️ THE TASK WAITS, NOT THE WORKER. A waiting task frees its worker's Active
 * slot, so the worker carries on with other work. Each item used to open with
 * "<worker> is waiting on you:", which read as an idle worker while RCG
 * Networks was busy (operator, 2026-10-20).
 */
export default function OperatorWaitCard({ waits, busy = false, onDone, onOpenTask }: Props) {
  if (waits.length === 0) return null;
  return (
    <section className="queen-attention-card operator-wait-card" aria-labelledby="operator-wait-heading">
      <div>
        <p className="eyebrow">Waiting on you</p>
        <h3 id="operator-wait-heading">
          {waits.length === 1 ? "1 task needs you to do something" : `${waits.length} tasks need you to do something`}
        </h3>
        <p className="muted">
          The tasks are paused, not their workers, who carry on with other work. Done sends a task back to its worker.
        </p>
        <ul className="operator-wait-list">
          {waits.map((wait) => (
            <li key={wait.task_id}>
              <p>
                <strong>Needed from you:</strong> {wait.needs}
              </p>
              <p className="muted">{wait.worker_name ? `${wait.worker_name} · ${wait.task_title}` : wait.task_title}</p>
              <div className="operator-wait-actions">
                <button type="button" className="primary-button" disabled={busy} onClick={() => onDone(wait.task_id)}>
                  Done
                </button>
                {onOpenTask && (
                  <button type="button" className="secondary-button" onClick={() => onOpenTask(wait.task_id)}>
                    Open the task
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
