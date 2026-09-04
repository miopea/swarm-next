import { authenticatedFetch } from "./request";

export type TaskState =
  | "draft"
  | "ready"
  | "active"
  | "blocked"
  | "review"
  /**
   * Finished and accepted, waiting only to ship.
   *
   * OPEN, not closed. The work is not done until it lands, and a recorded
   * deployment closes it without anybody clicking. Enrolling it in
   * CLOSED_TASK_STATES would hide work that has not shipped.
   */
  | "awaiting_release"
  | "completed"
  /** Closed for a reason other than success, so the evidence question never applies. */
  | "abandoned";

/**
 * States in which work is closed, whether or not it succeeded.
 *
 * ONE LIST, because the old test was `state !== "completed"` in six places and
 * every one of them silently meant "still open". Adding a second terminal
 * state would have enrolled abandoned work in every queue, count and worker
 * workload in the product, and nothing would have failed.
 */
export const CLOSED_TASK_STATES: readonly TaskState[] = ["completed", "abandoned"];

export function isClosedTaskState(state: TaskState): boolean {
  return CLOSED_TASK_STATES.includes(state);
}

export function isOpenTaskState(state: TaskState): boolean {
  return !isClosedTaskState(state);
}
/**
 * Who owes the next move on a task.
 *
 * Derived by the server from state and assignment, never stored, so it cannot
 * disagree with them. `blocked` is deliberately NOT `queen`: a hard block —
 * work waiting on another task — is not a move anyone here is failing to make,
 * and folding it into her queue would bury exactly the cases that need a
 * different kind of attention.
 */
export type NextMoveOwner = "worker" | "queen" | "operator" | "blocked" | "release" | "nobody";

export type TaskPriority = "low" | "normal" | "high" | "urgent";

export type Task = {
  id: string;
  hive_id: string;
  title: string;
  description: string;
  /**
   * One line from the operator about how this task should be approached rather
   * than what it contains — "interview me first", "analyse this, do not act on
   * it". Empty when the operator has not said anything.
   */
  operator_instruction: string;
  /** Whether anyone has recorded where this work is running. */
  deployment_recorded?: boolean;
  /** Deployment recorded, or a nothing-to-deploy claim Queen approved. Either closes a task. */
  closed_on_evidence?: boolean;
  /**
   * The operator recorded that this work cannot NOW be shown to be live.
   *
   * A third outcome, not a kind of evidence. It takes a task out of the
   * waiting-on-evidence queue because nothing is coming, and it must never be
   * rendered as verified: nobody checked, and the record says so.
   */
  closed_unverifiable?: boolean;
  /** Whether any Swarm worker has ever acted on this task. False for a Jira issue mirrored in. */
  worked_here?: boolean;
  priority: TaskPriority;
  workspace: string;
  state: TaskState;
  next_move_owner?: NextMoveOwner;
  assigned_worker_id: string | null;
  assigned_session_id: string | null;
  dispatch_state?: "queued" | "dispatching" | "delivered" | "uncertain" | null;
  outcome_delivery_state?: "queued" | "dispatching" | "delivered" | "uncertain" | null;
  position: number;
  created_at: number;
  updated_at: number;
};

export type TaskActivityKind = "created" | "details_updated" | "state_changed" | "assigned" | "unassigned" | "removed" | "restored" | "corrected" | "amended";
export type TaskActivityActorKind = "operator" | "worker" | "jira" | "email" | "system";

export type TaskActivity = {
  sequence: number;
  task_id: string;
  kind: TaskActivityKind;
  from_state: TaskState | null;
  to_state: TaskState | null;
  note: string;
  occurred_at: number;
  actor_kind: TaskActivityActorKind;
  actor_id: string | null;
};

export type TaskActivityPage = { events: TaskActivity[]; truncated: boolean };

export type TaskDraftInput = {
  title: string;
  description: string;
  priority: TaskPriority;
  worker_id: string;
};

export type TaskUpdateInput = Partial<Omit<TaskDraftInput, "worker_id">> & {
  workspace?: string;
  operator_instruction?: string;
};
export type TaskCreateInput = Omit<TaskDraftInput, "worker_id"> & { workspace: string };

export async function fetchTasks(operatorToken: string, signal?: AbortSignal): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks", { signal });
  return response.json() as Promise<Task[]>;
}

/**
 * Settled work — abandoned, or completed with evidence or a recorded
 * unverifiable closure. Deliberately NOT part of the board list.
 *
 * The control room reloads its whole snapshot on every task event, and settled
 * work is the large majority of a long-lived Hive: measured on the operator's
 * board, 462 of 561 tasks and 1,411 KB of the 1,711 KB. The board renders it
 * inside a collapsed panel, so it was reloaded constantly and looked at rarely.
 * This is fetched once per page load, and again when that panel is opened.
 */
export async function fetchSettledTasks(operatorToken: string): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks/settled");
  return response.json() as Promise<Task[]>;
}

export async function fetchTaskActivity(operatorToken: string, taskId: string, limit = 30): Promise<TaskActivityPage> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/activity?limit=${encodeURIComponent(String(limit))}`,
  );
  return response.json() as Promise<TaskActivityPage>;
}

export async function fetchRecentTaskActivity(operatorToken: string, limit = 100, signal?: AbortSignal): Promise<TaskActivityPage> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/activity?limit=${encodeURIComponent(String(limit))}`, { signal });
  return response.json() as Promise<TaskActivityPage>;
}

export async function reorderTasks(operatorToken: string, taskIds: string[]): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks/order", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ task_ids: taskIds }),
  });
  return response.json() as Promise<Task[]>;
}

export async function createTask(operatorToken: string, input: TaskCreateInput): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Task>;
}

export async function updateTask(operatorToken: string, taskId: string, input: TaskUpdateInput): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Task>;
}

export async function recordTaskUnverifiable(operatorToken: string, taskId: string, note: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/unverifiable`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ note }),
  });
}

export async function fetchRemovedTasks(operatorToken: string): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks/removed");
  return response.json() as Promise<Task[]>;
}

export async function removeTask(operatorToken: string, taskId: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}`, {
    method: "DELETE",
  });
}

export async function restoreTask(operatorToken: string, taskId: string): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/restore`, {
    method: "POST",
  });
  return response.json() as Promise<Task>;
}

export async function transitionTask(operatorToken: string, taskId: string, state: TaskState, note = ""): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/state`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ state, note }),
  });
  return response.json() as Promise<Task>;
}

export async function assignTask(operatorToken: string, taskId: string, workerId: string | null): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/${encodeURIComponent(taskId)}/assignment`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ worker_id: workerId }),
  });
  return response.json() as Promise<Task>;
}
