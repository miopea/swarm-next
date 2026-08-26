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
  system_role?: "scout";
  provider: ProviderKind;
  workspace: string;
  autostart: boolean;
  position: number;
  active_session_id: string | null;
  created_at: number;
  updated_at: number;
  running: boolean;
  attention_state: WorkerAttentionState;
  /** Something this worker started is still running after its turn ended. */
  background_work?: boolean;
  /** Wall-clock second this worker's terminal last produced output. */
  last_output_at?: number;
  /** When this worker's oldest unanswered request was filed, while it holds. */
  held_for_answer_since?: number;
  /** One of its unanswered requests has passed the deadline its asker set. */
  answer_overdue?: boolean;
  /** Swarm wrote a briefing to this worker and could not confirm it landed. */
  unconfirmed_delivery?: boolean;
  /** The device currently holding input and terminal geometry for this worker. */
  engaged_device_id?: string;
  engaged_device_class?: "desktop" | "mobile";
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
  workspace?: string;
  allow_outside_roots?: boolean;
};

export type RepositoryState = {
  branch: string | null;
  detached: boolean;
  changed_paths: number;
};

/**
 * Repository state for one worker. Scoped to a single worker deliberately: the
 * surface that shows it is per-selected-worker, so a large roster never turns
 * into one Git invocation per row.
 */
export async function fetchWorkerRepository(
  operatorToken: string,
  workerId: string,
): Promise<RepositoryState | null> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/workers/${encodeURIComponent(workerId)}/repository`,
  );
  return response.json() as Promise<RepositoryState | null>;
}

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

export type WorkerDescriptionDraft = {
  description: string;
  source: "repository_metadata" | "claude_review";
};

export async function draftWorkerDescription(operatorToken: string, workerId: string): Promise<WorkerDescriptionDraft> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/description-draft`, {
    method: "POST",
  });
  return response.json() as Promise<WorkerDescriptionDraft>;
}

export async function improveWorkerDescription(operatorToken: string, workerId: string): Promise<WorkerDescriptionDraft> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/description-improvement`, {
    method: "POST",
  });
  return response.json() as Promise<WorkerDescriptionDraft>;
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

/**
 * Opens a scratch shell in a worker's workspace.
 *
 * Returns a session id that is NOT a worker session: nothing binds it, so the
 * roster never shows it and sleeping the worker does not touch it. It borrows
 * the worker's workspace path and nothing else.
 */
export async function openWorkerShell(operatorToken: string, workerId: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/shell`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ rows: 24, columns: 80 }),
  });
  return ((await response.json()) as { session_id: string }).session_id;
}

export async function stopWorker(operatorToken: string, workerId: string): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/session`, {
    method: "DELETE",
  });
  return response.json() as Promise<Worker>;
}

/**
 * Claims a worker for this device without sending it anything.
 *
 * ADR 0049. Reclaiming a screen and instructing an agent are not the same act,
 * and until this they were the same button: the only way to take a worker back
 * from a device you had walked away from was to type into it, which sends real
 * input to a real provider.
 */
export async function claimWorker(
  operatorToken: string,
  workerId: string,
  deviceId: string,
): Promise<void> {
  await authenticatedFetch(
    operatorToken,
    `/api/v1/workers/${encodeURIComponent(workerId)}/engagement/${encodeURIComponent(deviceId)}`,
    { method: "POST" },
  );
}
