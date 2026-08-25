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
  /** Takes the worker back for this screen, without sending it anything. */
  onClaim?: () => void;
  /** Swarm wrote a briefing to this worker and could not confirm it landed. */
  unconfirmedDelivery?: boolean;
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
  onClaim,
  unconfirmedDelivery,
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
      {unconfirmedDelivery ? (
        <span
          className="worker-unconfirmed-detail"
          role="status"
          title="Swarm wrote a briefing to this worker and could not confirm the worker received it. Nothing is retried automatically, because a briefing delivered twice is worse than one the operator was told about."
        >Briefing unconfirmed — check the terminal below</span>
      ) : null}
      {engagement ? (
        <span
          className={`worker-engaged-elsewhere device-${engagement.deviceClass}`}
          role="status"
          title={engagement.detail}
        >{engagement.deviceClass === "mobile" ? "On phone" : "On another desktop"}</span>
      ) : null}
      {/* Naming the device that holds it and offering nothing to do about it
          left one remedy: type into the worker, which sends real input to a
          real provider. Reclaiming a screen is not instructing an agent. */}
      {engagement && onClaim ? (
        <button
          type="button"
          className="worker-claim"
          title="Show this worker here. It is not sent anything, and the other screen keeps running."
          onClick={onClaim}
        >Work here</button>
      ) : null}
      {openCount > 1 ? (
        <button
          type="button"
          className="worker-context-queue"
          aria-label={`Show all ${openCount} open tasks for ${worker.name}`}
          // Names whose work it is. Read beside a single task row, a bare
          // "1 active · 5 blocked · 1 ready" invites the reading that those
          // numbers describe THIS task.
          title={workSummary ? `${worker.name}'s open work: ${workSummary}` : undefined}
          onClick={() => onOpenQueue(worker.id)}
        >+{openCount - 1}</button>
      ) : null}
    </div>
  );
}
