import type { BlockedEscalation, HeldBriefing, RecoveryQueueItem } from "../api";
import { isOpenTaskState, prerequisiteSatisfied, type Task } from "../api/tasks";
import type { Worker } from "../api/workers";

/** Presentation only: roster order, then the dispatcher's recorded position/id order. */
export function groupQueueByWorker(tasks: Task[], workers: Worker[]) {
  const roster = new Map(workers.map(worker => [worker.id, worker]));
  const groups = new Map<string | null, Task[]>();
  for (const task of tasks) {
    const id = task.assigned_worker_id ?? null;
    const group = groups.get(id) ?? [];
    group.push(task);
    groups.set(id, group);
  }
  const compareId = (a: string, b: string) => a < b ? -1 : a > b ? 1 : 0;
  return [...groups].map(([workerId, items]) => ({
    workerId,
    name: workerId == null ? "Unassigned" : roster.get(workerId)?.name ?? `Worker unavailable (${workerId})`,
    tasks: items.sort((a, b) => a.position - b.position || compareId(a.id, b.id)),
  })).sort((a, b) => {
    if (a.workerId == null) return 1;
    if (b.workerId == null) return -1;
    return (roster.get(a.workerId)?.position ?? Number.MAX_SAFE_INTEGER)
      - (roster.get(b.workerId)?.position ?? Number.MAX_SAFE_INTEGER)
      || compareId(a.workerId, b.workerId);
  });
}

/** Worker attention can reflect a pending answer or a provider prompt. */
export function workerAwaitingAnswer(task: Task, worker: Worker | undefined): boolean {
  return (task.state === "ready" || task.state === "active")
    && task.assigned_session_id != null && worker?.running === true
    && task.assigned_worker_id === worker.id
    && task.assigned_session_id === worker.active_session_id
    && worker.attention_state === "awaiting_operator";
}

export function ordinaryActiveWork(task: Task): boolean {
  return task.state === "active" && task.next_move_owner === "worker"
    && (task.prerequisites ?? []).every(prerequisiteSatisfied)
    && (task.dispatch_state == null || task.dispatch_state === "delivered");
}

/** One task-count definition for the navigation and rendered queue rows. */
export function projectTaskQueues(tasks: Task[], held: HeldBriefing[], blocked: BlockedEscalation[], workers: Worker[] = [], recovery: RecoveryQueueItem[] = []) {
  const known = new Map(tasks.map((task) => [task.id, task]));
  const workerById = new Map(workers.map(worker => [worker.id, worker]));
  // Coordinator and task payloads refresh independently. Never revive an old
  // task/session/revision or move operator-owned work into Queen's check list.
  const recoveryChecks = recovery.filter(item => {
    const task = known.get(item.task_id);
    return task?.next_move_owner === "worker" && isOpenTaskState(task.state)
      && task.assigned_worker_id === item.worker_id && task.assigned_session_id === item.session_id
      && task.updated_at === item.task_revision;
  });
  const recoveryIds = new Set(recoveryChecks.map(item => item.task_id));
  const ordinary = (task: Task) => ordinaryActiveWork(task)
    && !recoveryIds.has(task.id)
    && !workerAwaitingAnswer(task, workerById.get(task.assigned_worker_id ?? ""));
  const waitingTasks = tasks.filter((task) => isOpenTaskState(task.state) && !ordinary(task));
  const activeTasks = tasks.filter(ordinary);
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
  return { waitingTasks, activeTasks, heldBriefings, blockedWaits, extraBlockedWaits, recoveryChecks, taskCount: identities.size };
}
