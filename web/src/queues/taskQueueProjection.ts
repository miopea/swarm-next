import type { BlockedEscalation, HeldBriefing } from "../api";
import { isOpenTaskState, type Task } from "../api/tasks";

export function ordinaryActiveWork(task: Task): boolean {
  return task.state === "active" && task.next_move_owner === "worker"
    && (task.dispatch_state == null || task.dispatch_state === "delivered");
}

/** One task-count definition for the navigation and rendered queue rows. */
export function projectTaskQueues(tasks: Task[], held: HeldBriefing[], blocked: BlockedEscalation[]) {
  const known = new Map(tasks.map((task) => [task.id, task]));
  const waitingTasks = tasks.filter((task) => isOpenTaskState(task.state) && !ordinaryActiveWork(task));
  const activeTasks = tasks.filter(ordinaryActiveWork);
  // Independently refreshed coordinator snapshots must not resurrect work
  // that the current task snapshot already knows has moved on.
  const heldBriefings = held.filter((brief) => {
    const task = known.get(brief.task_id);
    return !task || ((task.state === "ready" || task.state === "active")
      && task.assigned_worker_id === brief.worker_id
      && task.dispatch_state !== "delivered" && task.dispatch_state !== "uncertain"
      && !ordinaryActiveWork(task));
  });
  const blockedWaits = blocked.filter((wait) => !known.has(wait.task_id) || known.get(wait.task_id)?.state === "blocked");
  const extraBlockedWaits = blockedWaits.filter((wait) => !known.has(wait.task_id));
  const identities = new Set([
    ...waitingTasks.map((task) => task.id),
    ...heldBriefings.map((brief) => brief.task_id),
    ...extraBlockedWaits.map((wait) => wait.task_id),
  ]);
  return { waitingTasks, activeTasks, heldBriefings, blockedWaits, extraBlockedWaits, taskCount: identities.size };
}
