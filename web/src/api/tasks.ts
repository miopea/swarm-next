import { authenticatedFetch } from "./request";

export type TaskState = "draft" | "ready" | "active" | "blocked" | "review" | "completed";
export type TaskPriority = "low" | "normal" | "high" | "urgent";

export type Task = {
  id: string;
  hive_id: string;
  title: string;
  description: string;
  priority: TaskPriority;
  workspace: string;
  state: TaskState;
  assigned_worker_id: string | null;
  assigned_session_id: string | null;
  dispatch_state?: "queued" | "dispatching" | "delivered" | "uncertain" | null;
  outcome_delivery_state?: "queued" | "dispatching" | "delivered" | "uncertain" | null;
  position: number;
  created_at: number;
  updated_at: number;
};

export type TaskActivityKind = "created" | "details_updated" | "state_changed" | "assigned" | "unassigned";
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

export type TaskUpdateInput = Partial<Omit<TaskDraftInput, "worker_id">> & { workspace?: string };
export type TaskCreateInput = Omit<TaskDraftInput, "worker_id"> & { workspace: string };

export async function fetchTasks(operatorToken: string): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks");
  return response.json() as Promise<Task[]>;
}

export async function fetchTaskActivity(operatorToken: string, taskId: string, limit = 30): Promise<TaskActivityPage> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/activity?limit=${encodeURIComponent(String(limit))}`,
  );
  return response.json() as Promise<TaskActivityPage>;
}

export async function fetchRecentTaskActivity(operatorToken: string, limit = 100): Promise<TaskActivityPage> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/tasks/activity?limit=${encodeURIComponent(String(limit))}`);
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
