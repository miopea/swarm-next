export type Health = { status: "ok"; version: string };
export const BROWSER_SESSION_AUTH = "browser-session-cookie";
const TRANSIENT_RUNTIME_STATUSES = new Set([502, 503, 504]);
export type SessionSummary = { session_id: string; running: boolean };
export type SessionsResponse = { type: "sessions"; sessions: SessionSummary[] };
export type SessionStartedResponse = { type: "session_started"; session_id: string };
export type TaskState = "draft" | "ready" | "active" | "blocked" | "review" | "completed";
export type TaskPriority = "low" | "normal" | "high" | "urgent";
export type WorkerRole = "queen" | "worker";
export type ProviderKind = "claude_code" | "codex";
export type ProviderCapabilities = { claude_code: boolean; codex: boolean };
export type PresentationDeviceClass = "desktop" | "mobile";
export type PresentationPreferences = {
  device_class: PresentationDeviceClass;
  color_theme: "light" | "dark";
  terminal_keys_visible: boolean;
  configured: boolean;
};
export type WorkerAttentionState = "sleeping" | "buzzing" | "with_operator" | "awaiting_operator" | "blocked";
export type ControlRoomEventKind = "tasks_changed" | "workers_changed" | "sessions_changed" | "runtime_changed" | "decisions_changed" | "presence_changed" | "notifications_changed";
export type PresenceMode = "at_hive" | "away" | "night_watch";
export type PresenceSource = "manual" | "active_device" | "screen_locked" | "inactive_device" | "timed_out";
export type PresenceDeviceClass = "desktop" | "mobile";
export type PresenceObservationState = "active" | "idle" | "locked" | "hidden";
export type OperatorPresence = { mode: PresenceMode; manual_mode: PresenceMode | null; source: PresenceSource };
export type NotificationPolicy = "important_only" | "all_decisions" | "off";
export type NotificationSettings = { policy: NotificationPolicy; subscription_count: number; vapid_public_key: string };
export type QueenAutonomyLevel = "advisory" | "coordinate" | "local_execution";
export type QueenAutonomyPolicy = { at_hive: QueenAutonomyLevel; away: QueenAutonomyLevel; night_watch: QueenAutonomyLevel };
export type ControlRoomEvent = { sequence: number; hive_id: string; kind: ControlRoomEventKind; occurred_at: number };
export type ControlRoomEventPage = { events: ControlRoomEvent[]; next_cursor: number; reset_required: boolean };
export type TerminalHostStatus = {
  protocol_version: number;
  host_version: string;
  draining: boolean;
  running_sessions: number;
  retained_sessions: number;
  resources?: { resident_memory_bytes: number | null } | null;
};
export type WorkerEngineMaintenanceResult = {
  previous_version: string;
  current_version: string;
  stopped_sessions: number;
  restarted_workers: number;
};
export type ResourcePressure = "normal" | "advisory" | "critical" | "unavailable";
export type RuntimeResources = {
  sampled_at: number;
  policy: { mode: "observe_only"; advisory_bytes: number; critical_bytes: number };
  api: { resident_memory_bytes: number | null; pressure: ResourcePressure };
  terminal_host: { resident_memory_bytes: number | null; pressure: ResourcePressure };
};
export type HistoryDiagnostics = {
  retained_bytes: number;
  session_count: number;
  segment_count: number;
  dropped_records: number;
  dropped_bytes: number;
  recovered_truncated_bytes: number;
  recovered_corrupt_segments: number;
};
export type HiveIdentity = {
  operator: { id: string; display_name: string };
  hive: { id: string; name: string; operator_id: string; apiary_id: string | null };
};
export type DogfoodReport = {
  id: string;
  expectation: string;
  observation: string;
  diagnostic_bundle: string;
  attachment_name: string | null;
  created_at: number;
};
export type JiraConnectionState = "not_connected" | "ready" | "network_unavailable" | "credentials_invalid" | "permission_denied";
export type JiraReadiness = {
  configured: boolean;
  connection: JiraConnectionState;
  account_name: string | null;
};
export type JiraProject = { id: string; key: string; name: string };
export type JiraProjectStatus = {
  id: string;
  name: string;
  category_key: string;
  recommended_task_state: TaskState;
};
export type JiraProjectBinding = {
  id: string;
  project_id: string;
  project_key: string;
  project_name: string;
  scope: "hive" | "apiary";
  hive_id: string;
  apiary_id: string | null;
  access_verified: boolean;
  workflow_mapped: boolean;
};
export type JiraStatusMapping = {
  jira_status_id: string;
  jira_status_name: string;
  task_state: TaskState;
};

export type Worker = {
  id: string;
  hive_id: string;
  name: string;
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

export type DecisionRequest = {
  id: string;
  hive_id: string;
  requesting_worker_id: string;
  task_id: string | null;
  kind: "input" | "approval" | "credentials" | "conflict" | "help";
  urgency: "normal" | "time_sensitive";
  title: string;
  reason: string;
  risk: string;
  evidence: string;
  suggested_action: string;
  allowed_actions: string[];
  deadline: number | null;
  state: "pending" | "resolved";
  resolution_action: string | null;
  resolution_note: string;
  resolved_by_operator_id: string | null;
  created_at: number;
  updated_at: number;
  resolved_at: number | null;
  delivery_state: "queued" | "dispatching" | "delivered" | "uncertain" | null;
};

export type TaskActivityKind = "created" | "details_updated" | "state_changed" | "assigned" | "unassigned";

export type TaskActivity = {
  sequence: number;
  task_id: string;
  kind: TaskActivityKind;
  from_state: TaskState | null;
  to_state: TaskState | null;
  note: string;
  occurred_at: number;
};

export type TaskActivityPage = {
  events: TaskActivity[];
  truncated: boolean;
};

export type TaskDraftInput = {
  title: string;
  description: string;
  priority: TaskPriority;
  worker_id: string;
};

export type TaskUpdateInput = Partial<Omit<TaskDraftInput, "worker_id">> & { workspace?: string };
export type TaskCreateInput = Omit<TaskDraftInput, "worker_id"> & { workspace: string };

export async function fetchControlRoomEvents(
  operatorToken: string,
  after: number,
  signal?: AbortSignal,
): Promise<ControlRoomEventPage> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/control-room/events?after=${encodeURIComponent(String(after))}`,
    { signal },
  );
  return response.json() as Promise<ControlRoomEventPage>;
}

export async function fetchPresence(operatorToken: string): Promise<OperatorPresence> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence");
  return response.json() as Promise<OperatorPresence>;
}

export async function setManualPresence(
  operatorToken: string,
  manualMode: PresenceMode | null,
): Promise<OperatorPresence> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ manual_mode: manualMode }),
  });
  return response.json() as Promise<OperatorPresence>;
}

export async function observePresence(
  operatorToken: string,
  deviceId: string,
  deviceClass: PresenceDeviceClass,
  state: PresenceObservationState,
): Promise<OperatorPresence> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/presence/devices/${encodeURIComponent(deviceId)}`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ device_class: deviceClass, state }),
    },
  );
  return response.json() as Promise<OperatorPresence>;
}
export async function fetchNotificationSettings(operatorToken: string): Promise<NotificationSettings> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/notifications/settings");
  return response.json() as Promise<NotificationSettings>;
}

export async function setNotificationPolicy(
  operatorToken: string,
  policy: NotificationPolicy,
): Promise<NotificationSettings> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/notifications/settings", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ policy }),
  });
  return response.json() as Promise<NotificationSettings>;
}

export async function saveNotificationSubscription(
  operatorToken: string,
  deviceId: string,
  input: { device_class: PresenceDeviceClass; endpoint: string; keys: { p256dh: string; auth: string } },
): Promise<NotificationSettings> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/notifications/subscriptions/${encodeURIComponent(deviceId)}`,
    { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(input) },
  );
  return response.json() as Promise<NotificationSettings>;
}

export async function removeNotificationSubscription(
  operatorToken: string,
  deviceId: string,
): Promise<NotificationSettings> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/notifications/subscriptions/${encodeURIComponent(deviceId)}`,
    { method: "DELETE" },
  );
  return response.json() as Promise<NotificationSettings>;
}

export async function sendTestNotification(operatorToken: string, deviceId: string): Promise<NotificationSettings> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/notifications/subscriptions/${encodeURIComponent(deviceId)}/test`,
    { method: "POST" },
  );
  return response.json() as Promise<NotificationSettings>;
}
export async function fetchTerminalHostStatus(operatorToken: string): Promise<TerminalHostStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/terminal-host");
  const payload = (await response.json()) as { type: "host_status"; status: TerminalHostStatus };
  return payload.status;
}

export async function fetchRuntimeResources(operatorToken: string): Promise<RuntimeResources> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/resources");
  return response.json() as Promise<RuntimeResources>;
}

export async function fetchHistoryDiagnostics(operatorToken: string): Promise<HistoryDiagnostics | null> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/history/diagnostics");
  const payload = (await response.json()) as { type: "history_diagnostics"; diagnostics: HistoryDiagnostics | null };
  return payload.diagnostics;
}

export async function fetchHive(operatorToken: string): Promise<HiveIdentity> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/hive");
  return response.json() as Promise<HiveIdentity>;
}

export async function fetchSessions(operatorToken: string): Promise<SessionSummary[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/sessions");
  const payload = (await response.json()) as SessionsResponse;
  return payload.sessions;
}

export async function fetchWorkers(operatorToken: string): Promise<Worker[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workers");
  return response.json() as Promise<Worker[]>;
}

export async function fetchWorkspaces(operatorToken: string): Promise<WorkspaceChoice[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workspaces");
  return response.json() as Promise<WorkspaceChoice[]>;
}

export async function fetchTasks(operatorToken: string): Promise<Task[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks");
  return response.json() as Promise<Task[]>;
}

export async function fetchDecisions(operatorToken: string): Promise<DecisionRequest[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/decisions");
  return response.json() as Promise<DecisionRequest[]>;
}

export async function resolveDecision(
  operatorToken: string,
  decisionId: string,
  action: string,
  note = "",
): Promise<DecisionRequest> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/decisions/${encodeURIComponent(decisionId)}/resolution`,
    { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ action, note }) },
  );
  return response.json() as Promise<DecisionRequest>;
}
export async function fetchTaskActivity(
  operatorToken: string,
  taskId: string,
  limit = 30,
): Promise<TaskActivityPage> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/activity?limit=${encodeURIComponent(String(limit))}`,
  );
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

export async function createTask(
  operatorToken: string,
  input: TaskCreateInput,
): Promise<Task> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/tasks", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Task>;
}

export async function updateTask(
  operatorToken: string,
  taskId: string,
  input: TaskUpdateInput,
): Promise<Task> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}`,
    {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(input),
    },
  );
  return response.json() as Promise<Task>;
}

export async function transitionTask(
  operatorToken: string,
  taskId: string,
  state: TaskState,
  note = "",
): Promise<Task> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/state`,
    {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ state, note }),
    },
  );
  return response.json() as Promise<Task>;
}

export async function assignTask(
  operatorToken: string,
  taskId: string,
  workerId: string,
): Promise<Task> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/tasks/${encodeURIComponent(taskId)}/assignment`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ worker_id: workerId }),
    },
  );
  return response.json() as Promise<Task>;
}

export async function createWorker(
  operatorToken: string,
  input: { name: string; workspace: string; provider?: ProviderKind; autostart?: boolean },
): Promise<Worker> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/workers", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<Worker>;
}

export async function updateWorker(
  operatorToken: string,
  workerId: string,
  input: { name?: string; autostart?: boolean },
): Promise<Worker> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/workers/${encodeURIComponent(workerId)}`,
    {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(input),
    },
  );
  return response.json() as Promise<Worker>;
}

export async function reorderWorkers(operatorToken: string, workerIds: string[]): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/workers/order", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ worker_ids: workerIds }),
  });
}

export async function startWorker(
  operatorToken: string,
  workerId: string,
): Promise<Worker> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/workers/${encodeURIComponent(workerId)}/start`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ rows: 24, columns: 80 }),
    },
  );
  return response.json() as Promise<Worker>;
}

export async function stopWorker(
  operatorToken: string,
  workerId: string,
): Promise<Worker> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/workers/${encodeURIComponent(workerId)}/session`,
    { method: "DELETE" },
  );
  return response.json() as Promise<Worker>;
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

export async function saveDogfoodReport(
  operatorToken: string,
  report: Omit<DogfoodReport, "id" | "created_at">,
): Promise<DogfoodReport> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/feedback/reports", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(report),
  });
  return response.json() as Promise<DogfoodReport>;
}

export async function fetchDogfoodReports(
  operatorToken: string,
  limit = 5,
): Promise<DogfoodReport[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/feedback/reports?limit=${encodeURIComponent(String(limit))}`,
  );
  return response.json() as Promise<DogfoodReport[]>;
}

export async function downloadDogfoodScreenshot(
  operatorToken: string,
  attachmentName: string,
): Promise<Blob> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/feedback/attachments/${encodeURIComponent(attachmentName)}`,
  );
  return response.blob();
}

export async function fetchJiraReadiness(operatorToken: string): Promise<JiraReadiness> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/readiness");
  return response.json() as Promise<JiraReadiness>;
}

export async function beginJiraAuthorization(operatorToken: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/auth/start", {
    method: "POST",
  });
  const result = await response.json() as { authorization_url: string };
  return result.authorization_url;
}

export async function disconnectJira(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/auth", { method: "DELETE" });
}

export async function fetchJiraProjects(operatorToken: string, query = ""): Promise<JiraProject[]> {
  const params = new URLSearchParams();
  if (query.trim()) params.set("query", query.trim());
  const suffix = params.size ? `?${params}` : "";
  const response = await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/projects${suffix}`);
  return response.json() as Promise<JiraProject[]>;
}

export async function fetchJiraProjectStatuses(operatorToken: string, projectIdOrKey: string): Promise<JiraProjectStatus[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/projects/${encodeURIComponent(projectIdOrKey)}/statuses`,
  );
  return response.json() as Promise<JiraProjectStatus[]>;
}

export async function fetchJiraBindings(operatorToken: string): Promise<JiraProjectBinding[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/bindings");
  return response.json() as Promise<JiraProjectBinding[]>;
}

export async function createJiraBinding(
  operatorToken: string,
  project: JiraProject,
): Promise<JiraProjectBinding> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/bindings", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      project_id: project.id,
      project_key: project.key,
      project_name: project.name,
    }),
  });
  return response.json() as Promise<JiraProjectBinding>;
}

export async function replaceJiraMappings(
  operatorToken: string,
  bindingId: string,
  mappings: JiraStatusMapping[],
): Promise<JiraStatusMapping[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/mappings`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mappings }),
    },
  );
  return response.json() as Promise<JiraStatusMapping[]>;
}

export async function syncJiraBinding(operatorToken: string, bindingId: string): Promise<Task[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/sync`,
    { method: "POST" },
  );
  return response.json() as Promise<Task[]>;
}

export async function uploadDogfoodScreenshot(operatorToken: string, image: File): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/feedback/attachments", {
    method: "POST",
    headers: { "Content-Type": image.type },
    body: image,
  });
  return ((await response.json()) as { name: string }).name;
}

export async function updateWorkerEngine(operatorToken: string): Promise<WorkerEngineMaintenanceResult> {
  const response = await authenticatedFetch(
    operatorToken,
    "/api/v1/runtime/terminal-host/maintenance",
    { method: "POST" },
  );
  return response.json() as Promise<WorkerEngineMaintenanceResult>;
}

export async function fetchProviderCapabilities(operatorToken: string): Promise<ProviderCapabilities> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/providers");
  return response.json() as Promise<ProviderCapabilities>;
}

export async function fetchPresentationPreferences(
  operatorToken: string,
  deviceClass: PresentationDeviceClass,
): Promise<PresentationPreferences> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/preferences/presentation/${deviceClass}`);
  return response.json() as Promise<PresentationPreferences>;
}

export async function setPresentationPreferences(
  operatorToken: string,
  preferences: Omit<PresentationPreferences, "configured">,
): Promise<PresentationPreferences> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/preferences/presentation/${preferences.device_class}`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        color_theme: preferences.color_theme,
        terminal_keys_visible: preferences.terminal_keys_visible,
      }),
    },
  );
  return response.json() as Promise<PresentationPreferences>;
}

export async function fetchQueenAutonomyPolicy(operatorToken: string): Promise<QueenAutonomyPolicy> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/queen-policy");
  return response.json() as Promise<QueenAutonomyPolicy>;
}

export async function setQueenAutonomyPolicy(
  operatorToken: string,
  policy: QueenAutonomyPolicy,
): Promise<QueenAutonomyPolicy> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/queen-policy", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(policy),
  });
  return response.json() as Promise<QueenAutonomyPolicy>;
}

export async function downloadDatabaseBackup(operatorToken: string): Promise<Blob> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/backups/database");
  return response.blob();
}

export async function releaseWorkerEngagement(
  operatorToken: string,
  sessionId: string,
  deviceId: string,
): Promise<void> {
  await authenticatedFetch(
    operatorToken,
    `/api/v1/terminal/sessions/${encodeURIComponent(sessionId)}/engagements/${encodeURIComponent(deviceId)}`,
    { method: "DELETE" },
  );
}

export async function createBrowserSession(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/auth/session", { method: "POST" });
}

export async function validateBrowserSession(): Promise<void> {
  await authenticatedFetch(BROWSER_SESSION_AUTH, "/api/v1/auth/session");
}

export async function revokeBrowserSession(): Promise<void> {
  await authenticatedFetch(BROWSER_SESSION_AUTH, "/api/v1/auth/session", { method: "DELETE" });
}

export async function authenticatedFetch(
  operatorToken: string,
  url: string,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  if (operatorToken !== BROWSER_SESSION_AUTH) headers.set("Authorization", `Bearer ${operatorToken}`);
  const response = await fetch(url, { ...init, headers, cache: "no-store", credentials: "same-origin" });
  if (!response.ok) {
    let detail = "";
    try {
      const body = (await response.json()) as { message?: string };
      detail = body.message ? `: ${body.message}` : "";
    } catch {
      // Some infrastructure failures return an empty or non-JSON response.
    }
    throw new RuntimeRequestError(response.status, `Runtime request returned ${response.status}${detail}`);
  }
  return response;
}

export async function fetchHealth(): Promise<Health> {
  const response = await fetch("/health", { cache: "no-store" });
  if (!response.ok) throw new RuntimeRequestError(response.status, `Health returned ${response.status}`);
  return response.json() as Promise<Health>;
}

export class RuntimeRequestError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
    this.name = "RuntimeRequestError";
  }
}

export async function recoverTransientRuntime<T>(operation: () => Promise<T>, delays = [250, 500, 1_000, 2_000, 4_000, 8_000]): Promise<T> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      const retryable = error instanceof TypeError
        || (error instanceof RuntimeRequestError && TRANSIENT_RUNTIME_STATUSES.has(error.status));
      if (!retryable || attempt >= delays.length) throw error;
      await new Promise((resolve) => window.setTimeout(resolve, delays[attempt]));
    }
  }
}
