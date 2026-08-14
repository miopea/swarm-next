import type { Task, TaskState, Worker } from "../api";
import { workerAttention } from "../workers/workerAttention";

const dispatchLabels = {
  queued: "Briefing waits for a quiet moment",
  dispatching: "Briefing worker",
  delivered: "Worker briefed",
  uncertain: "Briefing uncertain — task remains authoritative",
} as const;

const outcomeDeliveryLabels = {
  queued: "Queen handoff waits for a quiet moment",
  dispatching: "Notifying Queen",
  delivered: "Queen notified",
  uncertain: "Queen handoff uncertain — task remains authoritative",
} as const;

export default function TaskAssignment({ task, workers, workerRunning, busy, onAssign, onOpenWorker, onTransition, onStartWorker }: {
  task: Task;
  workers: Worker[];
  workerRunning: boolean;
  busy: boolean;
  onAssign: (task: Task, workerId: string) => Promise<void>;
  onOpenWorker: (sessionId: string) => void;
  onTransition: (task: Task, state: TaskState) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
}) {
  const assignableWorkers = workers.filter((worker) => worker.role !== "queen");
  const targetWorker = assignableWorkers.find((worker) => worker.id === task.assigned_worker_id);

  return (
    <div className="task-assignment-cell">
      <div className="task-current-worker">
        <span className="task-meta-label">Swarm worker</span>
        {targetWorker?.active_session_id ? (
          <button type="button" className="task-owner task-owner-link" onClick={() => onOpenWorker(targetWorker.active_session_id!)}>{targetWorker.name}</button>
        ) : <strong className="task-owner">{targetWorker?.name ?? "Unassigned"}</strong>}
      </div>
      <div className="assignment-row">
        <label className="visually-hidden" htmlFor={`assignment-${task.id}`}>Assign Swarm worker</label>
        <select
          aria-label={`Assign Swarm worker for ${task.title}`}
          id={`assignment-${task.id}`}
          value={targetWorker?.id ?? ""}
          onChange={(event) => void onAssign(task, event.target.value)}
          disabled={busy || assignableWorkers.length === 0}
        >
          <option value="">Unassigned</option>
          {assignableWorkers.map((worker) => (
            <option key={worker.id} value={worker.id}>{worker.name} · {workerAttention(worker).compactLabel}</option>
          ))}
        </select>
      </div>
      {task.dispatch_state && <p className={`task-dispatch task-dispatch-${task.dispatch_state}`} role="status">{dispatchLabels[task.dispatch_state]}</p>}
      {task.outcome_delivery_state && <p className={`task-dispatch task-dispatch-${task.outcome_delivery_state}`} role="status">{outcomeDeliveryLabels[task.outcome_delivery_state]}</p>}
      {(targetWorker || task.state !== "ready") && (
        <PrimaryTaskAction task={task} workerRunning={workerRunning} targetWorker={targetWorker} busy={busy} onTransition={onTransition} onStartWorker={onStartWorker} />
      )}
    </div>
  );
}

function PrimaryTaskAction({ task, workerRunning, targetWorker, busy, onTransition, onStartWorker }: {
  task: Task;
  workerRunning: boolean;
  targetWorker: Worker | undefined;
  busy: boolean;
  onTransition: (task: Task, state: TaskState) => Promise<void>;
  onStartWorker: (task: Task) => Promise<void>;
}) {
  if (task.state === "draft") return <button disabled={busy} onClick={() => void onTransition(task, "ready")}>Mark ready</button>;
  if (task.state === "ready" && !workerRunning) return <button disabled={busy || !targetWorker} onClick={() => void onStartWorker(task)}>{targetWorker ? `Wake ${targetWorker.name}` : "Choose worker"}</button>;
  if (task.state === "ready") return <button disabled={busy} onClick={() => void onTransition(task, "active")}>Start work</button>;
  if (task.state === "active") return <button disabled={busy} onClick={() => void onTransition(task, "review")}>Send to review</button>;
  if (task.state === "blocked") return <button disabled={busy} onClick={() => void onTransition(task, "active")}>Resume work</button>;
  if (task.state === "review") return <button disabled={busy} onClick={() => void onTransition(task, "completed")}>Complete</button>;
  return null;
}
