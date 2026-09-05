import { prerequisiteSatisfied, type Task } from "../api/tasks";

export default function TaskPrerequisiteList({ task, workerNames, onOpenTask }: {
  task: Task;
  workerNames: Map<string, string>;
  onOpenTask?: (taskId: string) => void;
}) {
  const prerequisites = task.prerequisites ?? [];
  if (prerequisites.length === 0) return null;
  const unresolved = prerequisites.filter((item) => !prerequisiteSatisfied(item));
  const rows = <ul className="queue-prerequisite-list" aria-label="Task prerequisites">
    {prerequisites.map((item) => <li key={item.prerequisite_id}>
      <div>
        {item.removed || !onOpenTask ? <strong>{item.title}</strong> : <button type="button" onClick={() => onOpenTask(item.prerequisite_id)}>{item.title}</button>}
        <span className="queue-task-meta">{item.removed ? "Removed · Queen must reconcile" : prerequisiteSatisfied(item) ? "Completed" : item.state === "abandoned" ? "Abandoned · not satisfied" : item.state.replaceAll("_", " ")}
          {" · "}{item.assigned_worker_id ? workerNames.get(item.assigned_worker_id) ?? "Worker not in current roster" : "No worker assigned"}</span>
      </div>
      {item.reason.length <= 240 ? <p className="queue-task-meta">{item.reason}</p> : <details className="decision-argument"><summary>{item.reason.slice(0, 240)}…</summary><p className="decision-prose">{item.reason}</p></details>}
    </li>)}
  </ul>;
  if (unresolved.length === 0) return <div className="queue-prerequisites">
    {task.state === "blocked" && <p className="queue-task-meta">{task.next_move_owner === "operator"
      ? "Prerequisites completed · your decision is still needed"
      : "Prerequisites completed · Queen checks remaining blockers before resuming"}</p>}
    <details><summary>{prerequisites.length} completed prerequisite{prerequisites.length === 1 ? "" : "s"}</summary>{rows}</details>
  </div>;
  return <div className="queue-prerequisites">
    <p className="queue-task-meta">{unresolved.length} unresolved prerequisite{unresolved.length === 1 ? "" : "s"}{task.state === "active" ? " · Queen must reconcile; running work has not been stopped" : ""}</p>
    {rows}
  </div>;
}
