export type Health = { status: "ok"; version: string };
export type SessionSummary = { session_id: string; running: boolean };
export type SessionsResponse = { type: "sessions"; sessions: SessionSummary[] };
export type SessionStartedResponse = { type: "session_started"; session_id: string };
export type TaskState = "draft" | "ready" | "active" | "blocked" | "review" | "completed";

export type Task = {
  id: string;
  title: string;
  workspace: string;
  state: TaskState;
  assigned_session_id: string | null;
  created_at: number;
  updated_at: number;
};

export async function fetchSessions(operatorToken: string): Promise<SessionSummary[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/sessions");
  const payload = (await response.json()) as SessionsResponse;
  return payload.sessions;
}

export async function fetchTasks(operatorToken: string): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks");
  return response.json() as Promise<Task[]>;
}

export async function createTask(
  operatorToken: string,
  input: { title: string; workspace: string },
): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Task>;
}

export async function transitionTask(
  operatorToken: string,
  taskId: string,
  state: TaskState,
): Promise<Task> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/state`,
    {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ state }),
    },
  );
  return response.json() as Promise<Task>;
}

export async function assignTask(
  operatorToken: string,
  taskId: string,
  sessionId: string,
): Promise<Task> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/assignment`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ session_id: sessionId }),
    },
  );
  return response.json() as Promise<Task>;
}

export async function startClaudeSession(operatorToken: string, workspace: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/sessions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ workspace, rows: 24, columns: 80 }),
  });
  return ((await response.json()) as SessionStartedResponse).session_id;
}

export async function stopClaudeSession(operatorToken: string, sessionId: string): Promise<void> {
  await authenticatedFetch(
    operatorToken,
    `/api/v1/terminal/sessions/${encodeURIComponent(sessionId)}`,
    { method: "DELETE" },
  );
}

export async function authenticatedFetch(
  operatorToken: string,
  url: string,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${operatorToken}`);
  const response = await fetch(url, { ...init, headers, cache: "no-store" });
  if (!response.ok) {
    let detail = "";
    try {
      const body = (await response.json()) as { message?: string };
      detail = body.message ? `: ${body.message}` : "";
    } catch {
      // Some infrastructure failures return an empty or non-JSON response.
    }
    throw new Error(`Runtime request returned ${response.status}${detail}`);
  }
  return response;
}
