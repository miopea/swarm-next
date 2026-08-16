import { authenticatedFetch } from "./request";

export type WorkerRole = "queen" | "worker";
export type ProviderKind = "claude_code" | "codex";
export type WorkerAttentionState =
  | "sleeping"
  | "resting"
  | "buzzing"
  | "with_operator"
  | "awaiting_operator"
  | "blocked";

export type Worker = {
  id: string;
  hive_id: string;
  name: string;
  description?: string;
  role: WorkerRole;
  provider: ProviderKind;
  workspace: string;
  autostart: boolean;
  position: number;
  active_session_id: string | null;
  created_at: number;
  updated_at: number;
  running: boolean;
  attention_state: WorkerAttentionState;
  engagement_expires_at?: number;
  runtime_error?: string;
};

export type WorkspaceChoice = {
  name: string;
  path: string;
  kind: "repository" | "folder";
  configured_worker_id: string | null;
};

export type CreateWorkerInput = {
  name: string;
  workspace: string;
  provider?: ProviderKind;
  autostart?: boolean;
  allow_outside_roots?: boolean;
};

export type UpdateWorkerInput = {
  name?: string;
  description?: string;
  provider?: ProviderKind;
  autostart?: boolean;
};

export async function fetchWorkers(operatorToken: string): Promise<Worker[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workers");
  return response.json() as Promise<Worker[]>;
}

export async function fetchWorkspaces(operatorToken: string): Promise<WorkspaceChoice[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workspaces");
  return response.json() as Promise<WorkspaceChoice[]>;
}

export async function createWorker(operatorToken: string, input: CreateWorkerInput): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workers", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Worker>;
}

export async function updateWorker(operatorToken: string, workerId: string, input: UpdateWorkerInput): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Worker>;
}

export async function removeWorker(operatorToken: string, workerId: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}`, {
    method: "DELETE",
  });
}

export async function reorderWorkers(operatorToken: string, workerIds: string[]): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/workers/order", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ worker_ids: workerIds }),
  });
}

export async function startWorker(operatorToken: string, workerId: string): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/start`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ rows: 24, columns: 80 }),
  });
  return response.json() as Promise<Worker>;
}

export async function stopWorker(operatorToken: string, workerId: string): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/session`, {
    method: "DELETE",
  });
  return response.json() as Promise<Worker>;
}
