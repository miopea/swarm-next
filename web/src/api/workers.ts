import { authenticatedFetch } from "./request";

export type ReleaseNote = {
  summary: string;
  kind: string;
  needs_worker_engine_update: boolean;
};

export type ReleaseVersionNotes = {
  version: string;
  notes: ReleaseNote[];
};

export type ReleaseNotesResponse = {
  running_version: string;
  /** The release this Hive was updated FROM; null on a first install or a checkout reload. */
  previous_version: string | null;
  releases: ReleaseVersionNotes[];
};

export type WorkerRole = "queen" | "worker";
export type ProviderKind = "claude_code" | "codex" | "gemini" | "grok" | "opencode";
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
  /**
   * A role the Hive assigns rather than the operator — `scout` today.
   *
   * A STRING, NOT A UNION OF KNOWN VALUES, because the server's column is
   * `Option<&'static str>` and will emit whatever it is given. Typing it as
   * `"scout"` asserted that scout is the ONLY possible value, which made
   * `system_role !== "scout"` look like a correct exclusion of system workers
   * when it only ever excluded one of them — the divergence that would have
   * reproduced the reorder 409 the moment a second role was added.
   *
   * It also forced a cast to write a test for that case: expressing a state the
   * server can produce required `"archivist" as unknown as "scout"`. A cast
   * needed to describe reality is a type that is wrong.
   *
   * Nothing in the UI compares this to a literal. Both consumers ask only
   * whether it is set, which is the question that survives new roles.
   */
  system_role?: string;
  provider: ProviderKind;
  workspace: string;
  autostart: boolean;
  position: number;
  active_session_id: string | null;
  created_at: number;
  updated_at: number;
  running: boolean;
  attention_state: WorkerAttentionState;
  /**
   * Spawned beside another worker to try a second provider, and not yet adopted.
   *
   * Optional so a response from a build that predates it reads as false rather
   * than undefined-and-truthy in a menu condition.
   */
  ephemeral?: boolean;
  /** Something this worker started is still running after its turn ended. */
  background_work?: boolean;
  /** A wake is queued or in flight for this worker, since this unix time. */
  waking_since?: number;
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
  /**
   * The bee this worker wears. Absent means it is derived from the worker's id,
   * so a Hive that has never chosen one still shows everybody differently.
   */
  mark?: string | null;
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
  /** The bee this worker wears. An empty string returns it to the derived one. */
  mark?: string;
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

/** Providers an operator may spawn a temporary worker on. */
export const TEMPORARY_PROVIDERS = [
  { provider: "claude_code", label: "Claude", alpha: false },
  { provider: "codex", label: "Codex", alpha: false },
  // ALPHA, and the label is load-bearing rather than decorative. For these
  // three, provider_activity cannot read the terminal: their prompt glyphs are
  // unknown and none of their CLIs is installed to learn them from, so a worker
  // on one of them reads Unknown rather than resting or buzzing. It still gets
  // work delivered and still raises attention -- Unknown is handled end to end
  // -- but the roster cannot tell you it has finished a turn.
  //
  // They also start bare: no conversation resume, and like Codex no MCP
  // configuration, so they do not reach the swarm tools.
  { provider: "gemini", label: "Gemini", alpha: true },
  { provider: "grok", label: "Grok", alpha: true },
  { provider: "opencode", label: "OpenCode", alpha: true },
] as const;

/**
 * Spawns a temporary worker beside this one, on another provider.
 *
 * A throwaway sibling in the same workspace, not a second session on the parent
 * — two providers under one worker would break the one-session-per-worker
 * assumption that sleep/wake and briefing delivery rely on.
 */
export async function spawnTemporaryWorker(operatorToken: string, workerId: string, provider: string): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/temporary`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ provider }),
  });
  return response.json() as Promise<Worker>;
}

/**
 * Adopts a temporary worker into the Hive under a permanent name.
 *
 * A flag change rather than a re-creation, so it keeps its id and everything
 * already written against it still points at the same worker.
 */
export async function adoptWorker(operatorToken: string, workerId: string, name: string): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/workers/${encodeURIComponent(workerId)}/adoption`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });
  return response.json() as Promise<Worker>;
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

/**
 * What changed in the releases this Hive has installed.
 *
 * Read out of the running bundle rather than fetched from anywhere: the notes
 * ship inside the artifact whose hash the manifest signature covered, so they
 * are as trustworthy as the release and cost no second request.
 */
export async function fetchReleaseNotes(operatorToken: string): Promise<ReleaseNotesResponse> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/release/notes");
  return response.json() as Promise<ReleaseNotesResponse>;
}

/**
 * Says one thing to every worker with a live session.
 *
 * Returns who it reached AND who it could not: a worker with no session is
 * excluded from delivery rather than queued, so a caller that ignores `skipped`
 * will report a broadcast as having reached people it never touched.
 */
export async function broadcastToWorkers(
  operatorToken: string,
  body: string,
): Promise<{ broadcast_id: string; reached: number; skipped: number }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workers/broadcast", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ body }),
  });
  return response.json() as Promise<{ broadcast_id: string; reached: number; skipped: number }>;
}

/**
 * Which workers would resume a conversation that is no longer the newest, and
 * which ones Swarm could not check at all.
 *
 * Both are returned. An unknown reported as healthy is the failure this exists
 * to prevent.
 */
export async function fetchWorkerConversations(
  operatorToken: string,
): Promise<{ workers: { worker_id: string; name: string; freshness: { state: string; [key: string]: unknown } }[] }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workers/conversations");
  return response.json() as Promise<{ workers: { worker_id: string; name: string; freshness: { state: string; [key: string]: unknown } }[] }>;
}
