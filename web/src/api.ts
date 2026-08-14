export type Health = { status: "ok"; version: string };
export const BROWSER_SESSION_AUTH = "browser-session-cookie";
const TRANSIENT_RUNTIME_STATUSES = new Set([502, 503, 504]);
export type ProcessResources = {
  resident_memory_bytes: number | null;
  process_tree_resident_memory_bytes?: number | null;
  process_tree_process_count?: number | null;
};
export type SessionSummary = { session_id: string; running: boolean; resources?: ProcessResources | null };
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
export type WorkerAttentionState = "sleeping" | "resting" | "buzzing" | "with_operator" | "awaiting_operator" | "blocked";
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
  resources?: ProcessResources | null;
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
  api: ProcessResources & { pressure: ResourcePressure };
  terminal_host: ProcessResources & { pressure: ResourcePressure };
  machine?: {
    memory_total_bytes: number | null;
    memory_available_bytes: number | null;
    memory_used_percent: number | null;
    swap_total_bytes: number | null;
    swap_used_bytes: number | null;
    swap_used_percent: number | null;
    load_average: [number, number, number] | null;
    logical_cpus: number | null;
    memory_pressure_avg10: number | null;
    cpu_pressure_avg10: number | null;
    io_pressure_avg10: number | null;
    pressure: ResourcePressure;
  };
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
export type Apiary = {
  id: string;
  name: string;
  keeper_operator_id: string;
  shared_work_backend: "jira" | "native";
};
export type LocalApiaryContext = { mode: "personal" } | {
  mode: "federated";
  apiary: Apiary;
  local_role: "keeper" | "member";
};
export type ApiaryMember = {
  hive_id: string;
  hive_name: string;
  operator_id: string;
  operator_display_name: string;
  role: "keeper" | "member";
  is_local: boolean;
};
export type ApiaryCollapseReadiness = {
  active_hive_count: number;
  pending_invitation_count: number;
  active_stewardship_count: number;
  open_cross_hive_work_count: number;
  departed_node_count: number;
};
export type ApiaryJiraProject = {
  apiary_id: string;
  project_id: string;
  project_key: string;
  project_name: string;
  promoted_by_operator_id: string;
  promoted_at: number;
};
export type HiveConnectionCard = {
  payload: {
    schema_version: number;
    protocol_version: number;
    node_id: string;
    hive_id: string;
    hive_name: string;
    operator_id: string;
    operator_display_name: string;
    public_key: string;
    issued_at: number;
    expires_at: number;
  };
  signature: string;
};
export type ApiaryHiveCandidate = {
  apiary_id: string;
  node_id: string;
  hive_id: string;
  hive_name: string;
  operator_id: string;
  operator_display_name: string;
  public_key: string;
  card_issued_at: number;
  card_expires_at: number;
  pinned_by_operator_id: string;
  pinned_at: number;
  last_verified_at: number;
  invitation_pending?: boolean;
};
export type ApiaryInvitationBundle = {
  keeper_connection_card: HiveConnectionCard;
  invitation: {
    payload: {
      schema_version: number;
      protocol_version: number;
      invitation_id: string;
      apiary_id: string;
      apiary_name: string;
      shared_work_backend: "jira" | "native";
      required_policy_revision: number;
      promoted_project_catalog_digest: string;
      keeper_node_id: string;
      keeper_hive_id: string;
      keeper_operator_id: string;
      invited_node_id: string;
      invited_hive_id: string;
      invited_operator_id: string;
      keeper_endpoint: string;
      issued_at: number;
      expires_at: number;
      nonce: string;
    };
    signature: string;
  };
  promoted_projects: FederationProjectManifestEntry[];
  one_time_secret: string;
};
export type FederationProjectManifestEntry = {
  project_id: string;
  project_key: string;
  project_name: string;
};
export type FederationJoinInvitation = {
  invitation_id: string;
  apiary_id: string;
  apiary_name: string;
  shared_work_backend: "jira" | "native";
  required_policy_revision: number;
  promoted_project_catalog_digest: string;
  promoted_projects: FederationProjectManifestEntry[];
  keeper_node_id: string;
  keeper_hive_id: string;
  keeper_hive_name: string;
  keeper_operator_id: string;
  keeper_operator_display_name: string;
  keeper_endpoint: string;
  state: "keeper_pinned" | "policy_accepted" | "submitted" | "consumed" | "revoked" | "expired";
  imported_at: number;
  expires_at: number;
};
export type FederationProjectReadiness = {
  project: FederationProjectManifestEntry;
  binding_id: string | null;
  access_verified: boolean;
  workflow_mapped: boolean;
};
export type ApiaryJoinBlocker =
  | "hive_already_federated"
  | "invitation_required"
  | "invitation_expired"
  | "identity_not_verified"
  | "integration_not_ready"
  | "project_access_not_ready"
  | "policy_not_accepted"
  | "protocol_mismatch";
export type FederationJoinInvitationOverview = FederationJoinInvitation & {
  readiness: {
    jira_connection: JiraConnectionState;
    projects: FederationProjectReadiness[];
    blockers: ApiaryJoinBlocker[];
  };
  readiness_compatibility_fallback?: true;
};
export type HiveIdentity = {
  operator: { id: string; display_name: string };
  hive: { id: string; name: string; operator_id: string; apiary_id: string | null };
  apiary_context?: LocalApiaryContext;
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
  auto_sync_assigned: boolean;
};
export type JiraStatusMapping = {
  jira_status_id: string;
  jira_status_name: string;
  task_state: TaskState;
};
export type JiraIssue = {
  id: string;
  key: string;
  summary: string;
  description: string;
  status_id: string;
  status_name: string;
  assignee_account_id: string | null;
  assignee_name: string | null;
  updated_at: string;
};
export type JiraTaskLink = {
  issue_id: string;
  issue_key: string;
  issue_url: string | null;
  binding_id: string;
  project_key: string;
  project_name: string;
  task_id: string;
  jira_status_id: string;
  jira_status_name: string;
  jira_assignee_account_id: string | null;
  jira_assignee_name: string | null;
  remote_updated_at: string;
  last_synced_at: number;
  outbound_state: "queued" | "dispatching" | "conflict" | "uncertain" | null;
};
export type JiraComment = {
  id: string;
  author_name: string;
  body: string;
  created_at: string;
  updated_at: string;
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

export async function createApiary(
  operatorToken: string,
  name: string,
  sharedWorkBackend: "jira",
): Promise<LocalApiaryContext> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, shared_work_backend: sharedWorkBackend }),
  });
  return response.json() as Promise<LocalApiaryContext>;
}

export async function fetchApiaryMembers(operatorToken: string): Promise<ApiaryMember[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/members");
  return response.json() as Promise<ApiaryMember[]>;
}

export async function fetchHiveConnectionCard(
  operatorToken: string,
): Promise<HiveConnectionCard> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/connection-card");
  return response.json() as Promise<HiveConnectionCard>;
}

export async function fetchApiaryHiveCandidates(
  operatorToken: string,
): Promise<ApiaryHiveCandidate[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/hive-candidates");
  return response.json() as Promise<ApiaryHiveCandidate[]>;
}

export async function pinApiaryHiveCandidate(
  operatorToken: string,
  card: HiveConnectionCard,
): Promise<ApiaryHiveCandidate> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/hive-candidates", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(card),
  });
  return response.json() as Promise<ApiaryHiveCandidate>;
}

export async function inviteApiaryHiveCandidate(
  operatorToken: string,
  hiveId: string,
): Promise<ApiaryInvitationBundle> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/hive-candidates/${encodeURIComponent(hiveId)}/invitation`,
    { method: "POST" },
  );
  return response.json() as Promise<ApiaryInvitationBundle>;
}

export async function fetchFederationJoinInvitations(
  operatorToken: string,
): Promise<FederationJoinInvitationOverview[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/join-invitations");
  const invitations = await response.json() as FederationJoinInvitationOverview[];
  return invitations.map((invitation) => {
    if (invitation.readiness) return invitation;
    const blockers: ApiaryJoinBlocker[] = ["integration_not_ready"];
    if (invitation.promoted_projects.length > 0) blockers.push("project_access_not_ready");
    if (invitation.state !== "policy_accepted") blockers.push("policy_not_accepted");
    return {
      ...invitation,
      readiness: {
        jira_connection: "not_connected",
        projects: invitation.promoted_projects.map((project) => ({
          project,
          binding_id: null,
          access_verified: false,
          workflow_mapped: false,
        })),
        blockers,
      },
      readiness_compatibility_fallback: true,
    };
  });
}

export async function acceptFederationJoinPolicy(
  operatorToken: string,
  invitationId: string,
  policyRevision: number,
): Promise<FederationJoinInvitationOverview> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/join-invitations/${encodeURIComponent(invitationId)}/policy-acceptance`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ policy_revision: policyRevision }),
    },
  );
  return response.json() as Promise<FederationJoinInvitationOverview>;
}

export async function importFederationJoinInvitation(
  operatorToken: string,
  bundle: ApiaryInvitationBundle,
): Promise<FederationJoinInvitation> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/join-invitations", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(bundle),
  });
  return response.json() as Promise<FederationJoinInvitation>;
}

export async function fetchApiaryCollapseReadiness(
  operatorToken: string,
): Promise<ApiaryCollapseReadiness> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/collapse-readiness");
  return response.json() as Promise<ApiaryCollapseReadiness>;
}

export async function collapseApiary(operatorToken: string): Promise<LocalApiaryContext> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/collapse", {
    method: "POST",
  });
  return response.json() as Promise<LocalApiaryContext>;
}

export async function fetchApiaryJiraProjects(
  operatorToken: string,
): Promise<ApiaryJiraProject[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/jira-projects");
  return response.json() as Promise<ApiaryJiraProject[]>;
}

export async function promoteApiaryJiraProject(
  operatorToken: string,
  bindingId: string,
): Promise<ApiaryJiraProject> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/jira-projects/${encodeURIComponent(bindingId)}/promotion`,
    { method: "POST" },
  );
  return response.json() as Promise<ApiaryJiraProject>;
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
  workerId: string | null,
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
  input: { name: string; workspace: string; provider?: ProviderKind; autostart?: boolean; allow_outside_roots?: boolean },
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

export async function fetchJiraComments(
  operatorToken: string,
  taskId: string,
): Promise<JiraComment[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/comments`,
  );
  return response.json() as Promise<JiraComment[]>;
}

export async function addJiraComment(
  operatorToken: string,
  taskId: string,
  body: string,
): Promise<{ state: "queued" | "dispatching" | "delivered" | "conflict" | "uncertain" }> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/comments`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ body }),
    },
  );
  return response.json() as Promise<{ state: "queued" | "dispatching" | "delivered" | "conflict" | "uncertain" }>;
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

export async function fetchJiraTaskLinks(operatorToken: string): Promise<JiraTaskLink[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/task-links");
  const links = await response.json() as unknown;
  return Array.isArray(links) ? links as JiraTaskLink[] : [];
}

export async function retryJiraTaskLink(operatorToken: string, taskId: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/integrations/jira/task-links/${encodeURIComponent(taskId)}/retry`, {
    method: "POST",
  });
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

export async function fetchJiraMappings(operatorToken: string, bindingId: string): Promise<JiraStatusMapping[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/mappings`,
  );
  return response.json() as Promise<JiraStatusMapping[]>;
}

export async function setJiraAssignedSync(
  operatorToken: string,
  bindingId: string,
  enabled: boolean,
): Promise<JiraProjectBinding> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/assigned-sync`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled }),
    },
  );
  return response.json() as Promise<JiraProjectBinding>;
}

export async function fetchJiraBindingIssues(operatorToken: string, bindingId: string): Promise<JiraIssue[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/issues`,
  );
  return response.json() as Promise<JiraIssue[]>;
}

export async function syncJiraBinding(operatorToken: string, bindingId: string, issueIds: string[]): Promise<Task[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/integrations/jira/bindings/${encodeURIComponent(bindingId)}/sync`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ issue_ids: issueIds }),
    },
  );
  return response.json() as Promise<Task[]>;
}

export async function reconcileJira(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/integrations/jira/reconcile", { method: "POST" });
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
