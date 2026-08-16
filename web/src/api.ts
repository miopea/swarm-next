import {
  authenticatedFetch,
  BROWSER_SESSION_AUTH,
  recoverTransientRuntime,
  RuntimeRequestError,
} from "./api/request";
import type { PresenceDeviceClass } from "./api/presence";
import type { JiraConnectionState } from "./api/jira";
import type { TaskPriority, TaskState } from "./api/tasks";

export {
  authenticatedFetch,
  BROWSER_SESSION_AUTH,
  recoverTransientRuntime,
  RuntimeRequestError,
} from "./api/request";
export {
  fetchPresence,
  observePresence,
  setManualPresence,
} from "./api/presence";
export type {
  OperatorPresence,
  PresenceDeviceClass,
  PresenceMode,
  PresenceObservationState,
  PresenceSource,
} from "./api/presence";
export {
  createWorker,
  draftWorkerDescription,
  fetchWorkers,
  fetchWorkspaces,
  improveWorkerDescription,
  removeWorker,
  reorderWorkers,
  startWorker,
  stopWorker,
  updateWorker,
} from "./api/workers";
export {
  assignTask,
  createTask,
  fetchRecentTaskActivity,
  fetchTaskActivity,
  fetchTasks,
  reorderTasks,
  transitionTask,
  updateTask,
} from "./api/tasks";
export {
  addJiraComment,
  beginJiraAuthorization,
  createJiraBinding,
  disconnectJira,
  fetchJiraBindingIssues,
  fetchJiraBindings,
  fetchJiraComments,
  fetchJiraMappings,
  fetchJiraProjects,
  fetchJiraProjectStatuses,
  fetchJiraReadiness,
  fetchJiraTaskLinks,
  reconcileJira,
  replaceJiraMappings,
  retryJiraTaskLink,
  setJiraAssignedSync,
  syncJiraBinding,
} from "./api/jira";
export {
  beginEmailAuthorization,
  disconnectEmail,
  fetchEmailAttachmentPreview,
  fetchEmailConfiguration,
  fetchEmailInbox,
  fetchEmailMessage,
  fetchEmailReadiness,
  fetchEmailReply,
  fetchEmailTaskSource,
  fetchEmailTaskSources,
  fetchTaskDeployments,
  importEmailMessage,
  importEmailTask,
  prepareEmailReply,
  recordTaskDeployment,
  retryEmailReply,
  sendEmailReply,
  updateEmailConfiguration,
  updateEmailReplyDraft,
} from "./api/email";
export type {
  EmailAttachment,
  EmailConnectionState,
  EmailImport,
  EmailMessage,
  EmailMessageSummary,
  EmailOAuthConfiguration,
  EmailReadiness,
  EmailReply,
  EmailReplyState,
  EmailReplyTarget,
  EmailTaskAttachment,
  EmailTaskImportInput,
  EmailTaskSource,
  TaskDeployment,
} from "./api/email";
export type {
  JiraComment,
  JiraCommentDispatch,
  JiraConnectionState,
  JiraIssue,
  JiraProject,
  JiraProjectBinding,
  JiraProjectStatus,
  JiraReadiness,
  JiraStatusMapping,
  JiraTaskLink,
} from "./api/jira";
export type {
  Task,
  TaskActivity,
  TaskActivityActorKind,
  TaskActivityKind,
  TaskActivityPage,
  TaskCreateInput,
  TaskDraftInput,
  TaskPriority,
  TaskState,
  TaskUpdateInput,
} from "./api/tasks";
export type {
  CreateWorkerInput,
  ProviderKind,
  UpdateWorkerInput,
  Worker,
  WorkerAttentionState,
  WorkerRole,
  WorkspaceChoice,
} from "./api/workers";

export type Health = { status: "ok"; version: string; worker_engine_build_id?: string };
export type ProcessResources = {
  resident_memory_bytes: number | null;
  process_tree_resident_memory_bytes?: number | null;
  process_tree_process_count?: number | null;
};
export type SessionSummary = { session_id: string; running: boolean; resources?: ProcessResources | null };
export type SessionsResponse = { type: "sessions"; sessions: SessionSummary[] };
export type SessionStartedResponse = { type: "session_started"; session_id: string };
export type ProviderCapabilities = { claude_code: boolean; codex: boolean };
export type PresentationDeviceClass = "desktop" | "mobile";
export type PresentationPreferences = {
  device_class: PresentationDeviceClass;
  color_theme: "light" | "dark";
  terminal_keys_visible: boolean;
  configured: boolean;
};
export type ControlRoomEventKind = "tasks_changed" | "workers_changed" | "sessions_changed" | "runtime_changed" | "decisions_changed" | "presence_changed" | "notifications_changed";
export type NotificationPolicy = "important_only" | "all_decisions" | "off";
export type NotificationSettings = { policy: NotificationPolicy; subscription_count: number; vapid_public_key: string };
export type QueenAutonomyLevel = "advisory" | "coordinate" | "local_execution";
export type QueenAutonomyPolicy = { at_hive: QueenAutonomyLevel; away: QueenAutonomyLevel; night_watch: QueenAutonomyLevel };
export type ControlRoomEvent = { sequence: number; hive_id: string; kind: ControlRoomEventKind; occurred_at: number };
export type ControlRoomEventPage = { events: ControlRoomEvent[]; next_cursor: number; reset_required: boolean };
export type TerminalHostStatus = {
  protocol_version: number;
  host_version: string;
  host_build_id?: string | null;
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
export type DevelopmentRuntime = {
  enabled: boolean;
  version: string;
  state: "disabled" | "idle" | "requested" | "building" | "failed" | "ready";
  reload_available: boolean;
  source_revision: string | null;
  source_dirty: boolean;
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
export type StewardCapability =
  | "observe"
  | "assign"
  | "assist"
  | "takeover"
  | "manage_projects"
  | "manage_members";
export type Stewardship = {
  id: string;
  apiary_id: string;
  steward_operator_id: string;
  managed_hive_ids: string[];
  capabilities: StewardCapability[];
};
export type FederationStewardHiveObservation = {
  hive_id: string;
  ready_swarm_task_count: number;
  active_swarm_task_count: number;
  blocked_swarm_task_count: number;
  review_swarm_task_count: number;
  active_jira_claim_count: number;
  last_shared_activity_at: number | null;
};
export type FederationStewardshipSnapshot = {
  schema_version: number;
  protocol_version: number;
  apiary_id: string;
  member_node_id: string;
  member_operator_id: string;
  stewardship: Stewardship | null;
  observations?: FederationStewardHiveObservation[];
  generated_at: number;
};
export type FederationStewardTaskCommand = {
  id: string;
  apiary_id: string;
  target_hive_id: string;
  title: string;
  description: string;
  priority: TaskPriority;
  created_at: number;
};
export type FederationStewardTaskReceipt = {
  command_id: string;
  outcome: "applied" | "rejected";
  stewardship_id: string | null;
  task: ApiaryTask | null;
  processed_at: number;
};
export type FederationStewardTaskAuditEntry = {
  command_id: string;
  member_hive_id: string;
  member_operator_id: string;
  target_hive_id: string;
  stewardship_id: string | null;
  task_id: string | null;
  title: string;
  priority: TaskPriority;
  outcome: "applied" | "rejected";
  processed_at: number;
};
export type FederationStewardTaskOutboxEntry = {
  command: FederationStewardTaskCommand;
  state: "queued" | "applied" | "rejected";
  attempt_count: number;
  last_attempt_at: number | null;
  receipt: FederationStewardTaskReceipt | null;
};
export type ApiarySharedWorkClaim = {
  id: string;
  apiary_id: string;
  project_id: string;
  issue_id: string;
  issue_key: string;
  home_node_id: string;
  home_hive_id: string;
  home_operator_id: string;
  state: "reserved" | "confirmed";
  reserved_at: number;
  reservation_expires_at: number;
  confirmed_at: number | null;
  released_at: null;
  project_key: string;
  project_name: string;
  home_hive_name: string;
  home_operator_display_name: string;
};
export type FederationHandoffTarget = {
  node_id: string;
  hive_id: string;
  hive_name: string;
  operator_id: string;
  operator_display_name: string;
};
export type FederationClaimHandoff = {
  id: string;
  apiary_id: string;
  claim_id: string;
  project_id: string;
  issue_id: string;
  issue_key: string;
  source_node_id: string;
  source_hive_id: string;
  source_operator_id: string;
  target_node_id: string;
  target_hive_id: string;
  target_operator_id: string;
  state: "offered" | "accepted" | "completed" | "declined" | "cancelled";
  reason: string | null;
  offered_at: number;
  accepted_at: number | null;
  completed_at: number | null;
  closed_at: number | null;
};
export type FederationSyncCondition =
  | "idle"
  | "current"
  | "offline"
  | "authentication_required"
  | "incompatible";
export type FederationSyncHealth = {
  condition: FederationSyncCondition;
  last_attempt_at: number | null;
  last_success_at: number | null;
  consecutive_failures: number;
  next_attempt_at: number | null;
};
export type ApiaryTask = {
  id: string;
  apiary_id: string;
  source: "swarm";
  title: string;
  description: string;
  priority: TaskPriority;
  state: TaskState;
  home_node_id: string | null;
  home_hive_id: string | null;
  revision: number;
  created_at: number;
  updated_at: number;
};
export type LocalApiaryTaskExecution = {
  apiary_task_id: string;
  local_task_id: string;
  worker_id: string;
  state: TaskState;
  created_at: number;
};
export type FederationTaskSyncStatus = {
  cursor: number;
  task_count: number;
  last_applied_at: number | null;
};
export type FederationTaskCommandOutcome = "applied" | "conflict" | "rejected";
export type FederationTaskOutboxState = "queued" | FederationTaskCommandOutcome;
export type FederationTaskOutboxEntry = {
  command: {
    id: string;
    apiary_id: string;
    task_id: string;
    expected_revision: number;
    kind: "claim" | "transition";
    target_state: TaskState | null;
    created_at: number;
  };
  state: FederationTaskOutboxState;
  attempt_count: number;
  last_attempt_at: number | null;
  receipt: { command_id: string; outcome: FederationTaskCommandOutcome; task_revision: number | null; processed_at: number } | null;
};
export type FederationTaskOutboxStatus = {
  queued_count: number;
  conflict_count: number;
  rejected_count: number;
  last_attempt_at: number | null;
};
export type FederationTransportReadiness = {
  configured: boolean;
  endpoint: string | null;
  reachability: "remote_https" | "local_only" | "unconfigured";
};
export type FederationCatalogReadiness = {
  acknowledgement: {
    apiary_id: string;
    policy_revision: number;
    promoted_project_catalog_digest: string;
    project_count: number;
    snapshot_issued_at: number;
    snapshot_expires_at: number;
    acknowledged_at: number;
  } | null;
  jira_connection: JiraConnectionState;
  projects: FederationProjectReadiness[];
  blockers: ("catalog_missing" | "catalog_stale" | "integration_not_ready" | "policy_revision_changed" | "project_access_not_ready")[];
};
export type ApiaryCollapseReadiness = {
  active_hive_count: number;
  pending_invitation_count: number;
  active_stewardship_count: number;
  open_cross_hive_work_count: number;
  departed_node_count: number;
};
export type FederationDepartureReadiness = {
  apiary_id: string;
  member_node_id: string;
  member_hive_id: string;
  active_jira_claim_count: number;
  open_swarm_task_count: number;
  active_stewardship_count: number;
  pending_task_command_count: number;
  pending_jira_claim_count: number;
};
export type ApiaryDepartureStatus = {
  state: "active" | "departing";
  readiness: FederationDepartureReadiness;
  keeper_reachable: boolean;
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
export type ApiaryJoinLinkState = "open" | "awaiting_approval" | "approved" | "invitation_issued" | "revoked" | "expired";
export type ApiaryJoinLink = {
  id: string;
  apiary_id: string;
  apiary_name: string;
  keeper_endpoint: string;
  state: ApiaryJoinLinkState;
  candidate: ApiaryHiveCandidate | null;
  issued_at: number;
  expires_at: number;
};
export type ApiaryJoinLinkBundle = {
  link: ApiaryJoinLink;
  one_time_secret: string;
};
export type ApiaryKeeperLink = {
  link_id: string;
  keeper_endpoint: string;
  apiary_name: string | null;
  state: ApiaryJoinLinkState;
  created_at: number;
  updated_at: number;
  expires_at: number | null;
};
export type ApiaryKeeperJoinCapability = {
  link_id: string;
  keeper_endpoint: string;
  secret: string;
};
export type ApiaryKeeperLinkPoll = {
  link: ApiaryJoinLink;
  invitation_received: boolean;
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

export async function renameHive(operatorToken: string, name: string): Promise<HiveIdentity> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/hive", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });
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

export async function renameApiary(
  operatorToken: string,
  name: string,
): Promise<LocalApiaryContext> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  });
  return response.json() as Promise<LocalApiaryContext>;
}

export async function fetchApiaryMembers(operatorToken: string): Promise<ApiaryMember[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/members");
  return response.json() as Promise<ApiaryMember[]>;
}

export async function fetchApiaryStewardships(operatorToken: string): Promise<Stewardship[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/stewardships");
  return response.json() as Promise<Stewardship[]>;
}

export async function fetchApiaryStewardTaskAudit(
  operatorToken: string,
): Promise<FederationStewardTaskAuditEntry[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/steward-task-audit");
  return response.json() as Promise<FederationStewardTaskAuditEntry[]>;
}

export async function setApiaryStewardship(
  operatorToken: string,
  stewardOperatorId: string,
  managedHiveIds: string[],
  capabilities: StewardCapability[],
): Promise<Stewardship> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/stewardships/by-operator/${encodeURIComponent(stewardOperatorId)}`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ managed_hive_ids: managedHiveIds, capabilities }),
    },
  );
  return response.json() as Promise<Stewardship>;
}

export async function revokeApiaryStewardship(
  operatorToken: string,
  stewardshipId: string,
): Promise<void> {
  await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/stewardships/${encodeURIComponent(stewardshipId)}`,
    { method: "DELETE" },
  );
}

export async function fetchApiarySharedWork(
  operatorToken: string,
): Promise<ApiarySharedWorkClaim[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/shared-work");
  return response.json() as Promise<ApiarySharedWorkClaim[]>;
}

export async function fetchApiaryHandoffTargets(operatorToken: string): Promise<FederationHandoffTarget[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/handoff-targets");
  return response.json() as Promise<FederationHandoffTarget[]>;
}

export async function fetchApiaryClaimHandoffs(operatorToken: string): Promise<FederationClaimHandoff[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/handoffs");
  return response.json() as Promise<FederationClaimHandoff[]>;
}

export async function offerApiaryClaimHandoff(
  operatorToken: string,
  claimId: string,
  targetNodeId: string,
  reason: string,
): Promise<FederationClaimHandoff> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/claims/${encodeURIComponent(claimId)}/handoffs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ target_node_id: targetNodeId, reason: reason.trim() || null }),
  });
  return response.json() as Promise<FederationClaimHandoff>;
}

export async function acceptApiaryClaimHandoff(operatorToken: string, handoffId: string): Promise<FederationClaimHandoff> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/handoffs/${encodeURIComponent(handoffId)}/acceptance`, { method: "POST" });
  return response.json() as Promise<FederationClaimHandoff>;
}

export async function declineApiaryClaimHandoff(operatorToken: string, handoffId: string): Promise<FederationClaimHandoff> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/handoffs/${encodeURIComponent(handoffId)}/decline`, { method: "POST" });
  return response.json() as Promise<FederationClaimHandoff>;
}

export async function cancelApiaryClaimHandoff(operatorToken: string, handoffId: string): Promise<FederationClaimHandoff> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/handoffs/${encodeURIComponent(handoffId)}`, { method: "DELETE" });
  return response.json() as Promise<FederationClaimHandoff>;
}

export async function fetchFederationSyncHealth(
  operatorToken: string,
): Promise<FederationSyncHealth> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/sync-health");
  return response.json() as Promise<FederationSyncHealth>;
}

export async function fetchApiaryTasks(operatorToken: string): Promise<ApiaryTask[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/tasks");
  return response.json() as Promise<ApiaryTask[]>;
}

export async function createApiaryTask(
  operatorToken: string,
  input: { title: string; description: string; priority: TaskPriority; home_hive_id?: string | null },
): Promise<ApiaryTask> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/tasks", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<ApiaryTask>;
}

export async function fetchLocalApiaryTaskExecutions(
  operatorToken: string,
): Promise<LocalApiaryTaskExecution[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/tasks/local-executions");
  return response.json() as Promise<LocalApiaryTaskExecution[]>;
}

export async function materializeLocalApiaryTaskExecution(
  operatorToken: string,
  taskId: string,
  workerId: string,
): Promise<LocalApiaryTaskExecution> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/tasks/${encodeURIComponent(taskId)}/local-execution`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ worker_id: workerId }),
    },
  );
  return response.json() as Promise<LocalApiaryTaskExecution>;
}

export async function fetchFederationTaskSyncStatus(
  operatorToken: string,
): Promise<FederationTaskSyncStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/task-sync-status");
  return response.json() as Promise<FederationTaskSyncStatus>;
}

export async function fetchFederationTaskOutbox(operatorToken: string): Promise<FederationTaskOutboxEntry[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/task-outbox");
  return response.json() as Promise<FederationTaskOutboxEntry[]>;
}

export async function fetchFederationTaskOutboxStatus(operatorToken: string): Promise<FederationTaskOutboxStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/task-outbox-status");
  return response.json() as Promise<FederationTaskOutboxStatus>;
}

export async function claimApiaryTask(operatorToken: string, taskId: string): Promise<FederationTaskOutboxEntry> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/tasks/${encodeURIComponent(taskId)}/claim`, { method: "POST" });
  return response.json() as Promise<FederationTaskOutboxEntry>;
}

export async function transitionApiaryTask(operatorToken: string, taskId: string, targetState: TaskState): Promise<FederationTaskOutboxEntry> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/tasks/${encodeURIComponent(taskId)}/transition`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ target_state: targetState }),
  });
  return response.json() as Promise<FederationTaskOutboxEntry>;
}

export async function fetchFederationCatalogReadiness(
  operatorToken: string,
): Promise<FederationCatalogReadiness> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/catalog-readiness");
  return response.json() as Promise<FederationCatalogReadiness>;
}

export async function fetchMyFederationStewardship(
  operatorToken: string,
): Promise<FederationStewardshipSnapshot | null> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/my-stewardship");
  return response.json() as Promise<FederationStewardshipSnapshot | null>;
}

export async function fetchFederationStewardTaskOutbox(
  operatorToken: string,
): Promise<FederationStewardTaskOutboxEntry[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/steward/tasks");
  return response.json() as Promise<FederationStewardTaskOutboxEntry[]>;
}

export async function queueFederationStewardTask(
  operatorToken: string,
  input: { target_hive_id: string; title: string; description: string; priority: TaskPriority },
): Promise<FederationStewardTaskOutboxEntry> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/steward/tasks", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<FederationStewardTaskOutboxEntry>;
}

export async function fetchHiveConnectionCard(
  operatorToken: string,
): Promise<HiveConnectionCard> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/connection-card");
  return response.json() as Promise<HiveConnectionCard>;
}

export async function fetchFederationTransportReadiness(
  operatorToken: string,
): Promise<FederationTransportReadiness> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/transport-readiness");
  return response.json() as Promise<FederationTransportReadiness>;
}

export async function fetchApiaryHiveCandidates(
  operatorToken: string,
): Promise<ApiaryHiveCandidate[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/hive-candidates");
  return response.json() as Promise<ApiaryHiveCandidate[]>;
}

export async function createApiaryJoinLink(
  operatorToken: string,
): Promise<ApiaryJoinLinkBundle> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/join-links", {
    method: "POST",
  });
  return response.json() as Promise<ApiaryJoinLinkBundle>;
}

export async function fetchApiaryJoinLinks(
  operatorToken: string,
): Promise<ApiaryJoinLink[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/join-links");
  return response.json() as Promise<ApiaryJoinLink[]>;
}

export async function approveApiaryJoinLink(
  operatorToken: string,
  linkId: string,
): Promise<ApiaryJoinLink> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/join-links/${encodeURIComponent(linkId)}/approval`,
    { method: "POST" },
  );
  return response.json() as Promise<ApiaryJoinLink>;
}

export async function revokeApiaryJoinLink(
  operatorToken: string,
  linkId: string,
): Promise<ApiaryJoinLink> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/join-links/${encodeURIComponent(linkId)}`,
    { method: "DELETE" },
  );
  return response.json() as Promise<ApiaryJoinLink>;
}

export async function fetchApiaryKeeperLinks(
  operatorToken: string,
): Promise<ApiaryKeeperLink[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/keeper-links");
  return response.json() as Promise<ApiaryKeeperLink[]>;
}

export async function saveApiaryKeeperLink(
  operatorToken: string,
  capability: ApiaryKeeperJoinCapability,
): Promise<ApiaryKeeperLinkPoll> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/keeper-links", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(capability),
  });
  return response.json() as Promise<ApiaryKeeperLinkPoll>;
}

export async function pollApiaryKeeperLink(
  operatorToken: string,
  linkId: string,
): Promise<ApiaryKeeperLinkPoll> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/keeper-links/${encodeURIComponent(linkId)}/poll`,
    { method: "POST" },
  );
  return response.json() as Promise<ApiaryKeeperLinkPoll>;
}

export async function removeApiaryKeeperLink(
  operatorToken: string,
  linkId: string,
): Promise<void> {
  await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/keeper-links/${encodeURIComponent(linkId)}`,
    { method: "DELETE" },
  );
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

export async function joinFederationApiary(
  operatorToken: string,
  invitationId: string,
): Promise<LocalApiaryContext> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/apiary/join-invitations/${encodeURIComponent(invitationId)}/submission`,
    { method: "POST" },
  );
  return response.json() as Promise<LocalApiaryContext>;
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

export async function fetchApiaryDepartureStatus(
  operatorToken: string,
): Promise<ApiaryDepartureStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/departure-readiness");
  return response.json() as Promise<ApiaryDepartureStatus>;
}

export async function leaveApiary(operatorToken: string): Promise<LocalApiaryContext> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/departure", {
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

export async function fetchDevelopmentRuntime(operatorToken: string): Promise<DevelopmentRuntime> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/development");
  return response.json() as Promise<DevelopmentRuntime>;
}

export async function requestDevelopmentReload(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/runtime/development/reload", { method: "POST" });
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

export async function fetchHealth(): Promise<Health> {
  const response = await fetch("/health", { cache: "no-store" });
  if (!response.ok) throw new RuntimeRequestError(response.status, `Health returned ${response.status}`);
  return response.json() as Promise<Health>;
}
