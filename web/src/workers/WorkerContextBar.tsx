import type { RepositoryState, Task, Worker } from "../api";
import { repositoryName } from "./workerRoster";

export interface WorkerContextBarProps {
  worker: Worker;
  /** The task this worker is carrying now, and how much else it owns. */
  currentTask: Task;
  openCount: number;
  workSummary?: string;
  /** Absent while unread, `null` when the workspace is not a Git checkout. */
  repository?: RepositoryState | null;
  /** Set only when a device other than this one is driving the worker. */
  engagement?: { deviceClass: "desktop" | "mobile"; detail: string };
  taskStateLabel: (task: Task) => string;
  onOpenQueue: (workerId: string, focusTaskId?: string) => void;
}

/**
 * What the selected worker is doing, beside its terminal.
 *
 * Rendered once for the selected worker rather than inside each terminal: every
 * running terminal stays mounted, so a per-session copy would hold this state
 * once per worker in the roster to show one of them.
 */
export default function WorkerContextBar({
  worker,
  currentTask,
  openCount,
  workSummary,
  repository,
  engagement,
  taskStateLabel,
  onOpenQueue,
}: WorkerContextBarProps) {
  const repositoryDifference = repository?.changed_paths
    ? `, with ${repository.changed_paths} path(s) differing from HEAD`
    : "";
  return (
    <div className="worker-context" aria-label={`Work owned by ${worker.name}`}>
      <button
        type="button"
        className="worker-context-task"
        title={currentTask.title}
        onClick={() => onOpenQueue(worker.id, currentTask.id)}
      >
        <span className={`task-state state-${currentTask.state}`}>{taskStateLabel(currentTask)}</span>
        <span className="worker-context-title">{currentTask.title}</span>
      </button>
      {repository?.branch || repository?.detached ? (
        <span
          className="worker-repository"
          title={repository.detached
            ? `${repositoryName(worker.workspace)} has a detached HEAD${repositoryDifference}`
            : `${repositoryName(worker.workspace)} on ${repository.branch}${repository.changed_paths ? repositoryDifference : ", matching HEAD"}`}
        >
          <span className="worker-repository-branch">{repository.detached ? "detached" : repository.branch}</span>
          {repository.changed_paths ? (
            <span className="worker-repository-dirty">{repository.changed_paths}</span>
          ) : null}
        </span>
      ) : null}
      {engagement ? (
        <span
          className={`worker-engaged-elsewhere device-${engagement.deviceClass}`}
          role="status"
          title={engagement.detail}
        >{engagement.deviceClass === "mobile" ? "On phone" : "On another desktop"}</span>
      ) : null}
      {openCount > 1 ? (
        <button
          type="button"
          className="worker-context-queue"
          aria-label={`Show all ${openCount} open tasks for ${worker.name}`}
          title={workSummary}
          onClick={() => onOpenQueue(worker.id)}
        >+{openCount - 1}</button>
      ) : null}
    </div>
  );
}
