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
  fetchWorkerRepository,
  fetchWorkers,
  fetchWorkerConversations,
  broadcastToWorkers,
  fetchWorkspaces,
  adoptWorker,
  improveWorkerDescription,
  openWorkerShell,
  spawnTemporaryWorker,
  TEMPORARY_PROVIDERS,
  fetchReleaseNotes,
  removeWorker,
  reorderWorkers,
  startWorker,
  stopWorker,
  updateWorker,
  claimWorker,
} from "./api/workers";
export {
  assignTask,
  createTask,
  fetchRecentTaskActivity,
  recordTaskUnverifiable,
  fetchRemovedTasks,
  fetchTaskActivity,
  fetchTasks,
  fetchSettledTasks,
  reorderTasks,
  removeTask,
  restoreTask,
  transitionTask,
  updateTask,
} from "./api/tasks";
export {
  commitLegacyTaskMigration,
  commitLegacyWorkerMigration,
  listActiveLegacyWorkerMigrations,
  previewLegacyTaskMigration,
  previewLegacyWorkerMigration,
  rollbackLegacyTaskMigration,
  rollbackLegacyWorkerMigration,
} from "./api/migration";
export type {
  LegacyImportDisposition,
  LegacyMigrationBundle,
  LegacyMigrationPreview,
  LegacyMigrationReceipt,
  LegacyTaskPreview,
  LegacyWorkerImportDisposition,
  LegacyWorkerMigrationPreview,
  LegacyWorkerMigrationReceipt,
  LegacyWorkerPreview,
} from "./api/migration";
export {
  addJiraComment,
  beginJiraAuthorization,
  connectJiraWithApiToken,
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
  fetchJiraTaskDetail,
  fetchJiraTaskAttachment,
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
  fetchEmailTasksAwaitingReply,
  fetchEmailTaskAttachment,
  fetchTaskDeployments,
  importEmailMessage,
  importEmailTask,
  prepareEmailReply,
  recordTaskDeployment,
  retryEmailReply,
  reviseEmailReplyDraft,
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
  UnansweredEmailTask,
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
  JiraTaskDetail,
  JiraTaskAttachment,
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
  ReleaseNote,
  ReleaseNotesResponse,
  ReleaseVersionNotes,
  UpdateWorkerInput,
  RepositoryState,
  Worker,
  WorkerAttentionState,
  WorkerRole,
  WorkspaceChoice,
} from "./api/workers";

/** A subsystem that did not start, and why. */
export type DegradedSubsystem = {
  subsystem: string;
  reason: string;
};
export type Health = {
  /**
   * "degraded" means serving with something switched off, NOT failing.
   *
   * The HTTP status is 200 either way, deliberately: the updater's health
   * check reads only the status code, and a degraded Hive that answered an
   * error would fail every update and roll back — leaving an operator unable
   * to install their way out of a misconfiguration. That is the trap this
   * exists to prevent, so the distinction lives here rather than in the code.
   */
  status: "ok" | "degraded";
  version: string;
  worker_engine_build_id?: string;
  /** Largest image this Hive accepts; absent on older builds. */
  attachment_max_bytes?: number;
  /** What is switched off; absent on older builds, empty when nothing is. */
  degraded?: DegradedSubsystem[];
  /**
   * The protocol a prepared-but-deferred migration is waiting to activate.
   *
   * Absent when nothing is waiting, and on builds before 0.8.20. Carried on
   * health because it has to reach a Hive with development mode off.
   */
  protocol_migration_pending?: number;
};
export type ProcessResources = {
  resident_memory_bytes: number | null;
  process_tree_resident_memory_bytes?: number | null;
  process_tree_process_count?: number | null;
};
/** Startup provenance, not confirmation that the provider restored context. */
export type ConversationRecoveryAttempt = {
  recovery_id: string;
  number: number;
  step: { kind: "exact"; conversation: string } | { kind: "continue" } | { kind: "fresh" };
};
export type ConversationRecoveryOutcome =
  | { state: "restored"; conversation: string; via_continue: boolean }
  | { state: "fresh"; conversation: string }
  | { state: "manual"; reason: "provider_cannot_resume" | "uncertain_outcome" | "unexpected_conversation" | "fresh_start_failed" };
export type SessionSummary = { session_id: string; running: boolean; resources?: ProcessResources | null; recovery_attempt?: ConversationRecoveryAttempt | null; recovery_outcome?: ConversationRecoveryOutcome | null; confirmed_selection?: { revision: number; conversation: string } | null };
export type SessionsResponse = { type: "sessions"; sessions: SessionSummary[] };
export type SessionStartedResponse = { type: "session_started"; session_id: string };
/** A provider release on disk that some running workers have not picked up. */
export type SupersededProvider = {
  provider: "claude_code" | "codex" | "gemini" | "grok" | "opencode";
  version: string | null;
  installed_at: number | null;
  worker_ids: string[];
};
export type ProviderCapabilities = {
  claude_code: boolean;
  codex: boolean;
  /** Empty when every running worker is on the installed release. */
  superseded?: SupersededProvider[];
};
export type PresentationDeviceClass = "desktop" | "mobile";
export type PresentationPreferences = {
  device_class: PresentationDeviceClass;
  color_theme: "light" | "dark";
  terminal_keys_visible: boolean;
  configured: boolean;
};
export type ControlRoomEventKind = "tasks_changed" | "workers_changed" | "sessions_changed" | "runtime_changed" | "decisions_changed" | "presence_changed" | "notifications_changed";
export type NotificationPolicy = "important_only" | "all_decisions" | "off";
/** The screen Swarm opens on, chosen once for every device. */
export async function fetchStartSurface(operatorToken: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/preferences/start-surface");
  return ((await response.json()) as { start_surface: string }).start_surface;
}

export async function setStartSurface(operatorToken: string, startSurface: string): Promise<string> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/preferences/start-surface", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ start_surface: startSurface }),
  });
  return ((await response.json()) as { start_surface: string }).start_surface;
}

export type NotificationSettings = { policy: NotificationPolicy; subscription_count: number; vapid_public_key: string };
export type QueenAutonomyLevel = "advisory" | "coordinate" | "local_execution";
export type QueenAutonomyPolicy = { at_hive: QueenAutonomyLevel; away: QueenAutonomyLevel; night_watch: QueenAutonomyLevel };
export type QueenAutomationStatus = {
  enabled: boolean;
  state: "idle" | "queued" | "delivering" | "running" | "completed" | "uncertain";
  run_id: string | null;
  trigger: "actionable_work" | "manual" | null;
  actionable_count: number;
  attempts: number;
  requested_at: number | null;
  delivered_at: number | null;
  finished_at: number | null;
  outcome: "completed" | "needs_operator" | "no_action" | null;
  waiting_reason: string | null;
};
export type CoordinatorStatus = {
  completed_actions: number;
  queen_calls_avoided: number;
  uncertain_actions: number;
  queued_actions: number;
  stale_attention_actions: number;
  worker_exit_attention_actions: number;
  unstarted_attention_actions: number;
  last_action_at: number | null;
  automatic_start_admission: "allowed" | "deferred_advisory" | "deferred_critical" | "deferred_unavailable";
  automatic_start_batch_limit: number;
  /** What the coordinator is holding, once it has been true long enough to say. */
  held: HeldDelivery[];
  /** Blocks past the twelve hours the operator asked to hear about directly. */
  blocked_escalations?: BlockedEscalation[];
  /**
   * Finished work that nothing has settled, and why each one waits.
   *
   * NAMED FOR ITS SUBJECT rather than called "unverified", because that word
   * covers four different populations and picking the wrong one is how this
   * design started: a count of 49 that was really 31. This is work in review
   * carrying neither a deployment nor an approved exemption — everything the
   * coordinator can settle is already gone before this is computed.
   */
  unsettled_review?: UnsettledReview[];
  /**
   * Briefings queued and not yet handed over, with what each waits on.
   *
   * NOT AN ATTENTION ITEM, and that is measured rather than assumed. Every hold
   * reason is a queue behaving correctly — the worker is on something else, an
   * earlier task is ahead of it, or a person is typing in that terminal — and
   * they clear themselves: two on this Hive went to zero within minutes with
   * nothing done about them. Counting self-resolving states in "Needs you"
   * teaches the operator to distrust the badge, which is the one thing it
   * cannot afford.
   *
   * Shown anyway, because the server computed them and nothing rendered them:
   * a briefing held for hours means the task ahead of it never moved, and that
   * is a stall wearing a healthy label.
   */
  held_briefings?: HeldBriefing[];
};
export type UnsettledReview = {
  task_id: string;
  title: string;
  workspace: string;
  /**
   * Whose work this is — the operator's first question about waiting work.
   *
   * "Unassigned" when the task names no worker, or names one since archived.
   */
  worker_name: string;
  /**
   * Which state this row is in, as a stable label rather than prose:
   * "claim_unapproved", "code_no_deployment", "nothing_reported" or "settling".
   *
   * The card chips this and says `reason` once, because seven of eleven rows
   * carried the same forty-eight character sentence and it drowned the titles.
   */
  kind: string;
  /** Why a person is needed, derived from what the task recorded. */
  reason: string;
  /** When the work was filed. The only field on this row that carries age. */
  created_at: number;
};

export type HeldBriefing = {
  task_id: string;
  title: string;
  worker_id: string;
  worker_name: string;
  queued_at: number;
  /** Operator engagement, active work, queue order, or experimental_during_night_watch. */
  reason: string;
  /**
   * The earlier task this one is behind, when that is the hold.
   *
   * Without it "waiting its turn" is unfalsifiable — sixteen briefings reported
   * it at once on 2026-08-24 and named nothing to go and look at.
   */
  blocked_by?: string | null;
};
export type HeldDelivery = {
  /**
   * "delivery_held_open_prompt", "delivery_held_unsent_text" or
   * "wake_uncertain". Three situations with three different remedies: answer
   * the question, clear the line you typed, or wake the worker yourself.
   */
  kind: string;
  subject: string;
  worker_name: string | null;
  reason: string;
  first_observed_at: number;
  last_observed_at?: number;
  observations: number;
};
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
  state: "disabled" | "idle" | "requested" | "building" | "failed" | "ready" | "source_mismatch"
    /** Progress stopped, or the paths it reports progress to do not exist. */
    | "stalled" | "unavailable";
  reload_available: boolean;
  deployed_source_revision?: string | null;
  source_revision: string | null;
  source_dirty: boolean;
  /** Whether the running revision exists on a remote, and so survives losing this machine. */
  deployed_source_published: boolean;
  /**
   * Whether the worker engine is behind the running API.
   *
   * A reload deliberately does NOT restart the terminal host — that is what
   * stops it killing every worker's terminal mid-turn — so a reload can leave
   * the engine behind. null means the host could not be asked, which is a
   * different claim from being up to date.
   */
  worker_engine_update_required?: boolean | null;
  /**
   * How many worker sessions are running, from the same host snapshot.
   *
   * A plain count. The card phrases what it costs; this only says how many there
   * are, so the number cannot drift from the flag it is shown beside.
   */
  running_worker_sessions?: number | null;
  /**
   * Whether the checkout changes the terminal-host protocol.
   *
   * A reload CANNOT install this. It leaves the terminal host running on
   * purpose — that is what keeps worker terminals alive across a reload — so a
   * new protocol needs both processes swapped together, which stops every
   * worker. That makes it a decision about timing, and the operator cannot make
   * it if nothing says it is pending.
   */
  protocol_migration_required?: boolean | null;
  /** Which step failed: "build", "install", "protocol-change", or absent. */
  failure_reason?: string | null;
  /** The failing step's own last words, one bounded line. */
  failure_detail?: string | null;
};
/** One release the origin currently offers. Absent from the manifest means withdrawn. */
export type ReleaseOffer = {
  version: string;
  protocol: string;
  artifact_url: string;
  artifact_sha256: string;
  artifact_bytes: number;
  worker_engine_build_id: string;
  notes_url: string | null;
};
export type ReleaseStatus = {
  /** False when this build has no verifying key or no origin: the path is inert, not broken. */
  available: boolean;
  /** "unset" is a Hive nobody asked, which is not the same as one that said no. */
  mode: "unset" | "off" | "daily";
  current_version: string;
  development_build: boolean;
  last_checked_at: number | null;
  last_outcome: "offered" | "current" | "unreachable" | "rejected" | null;
  offer: ReleaseOffer | null;
  upgrade_available: boolean;
  /**
   * Whether the release carries a different worker engine. NOT "installing
   * stops your workers" — the install preserves the running terminal host and
   * the engine is swapped later, when sessions are idle.
   */
  carries_new_worker_engine: boolean;
  /**
   * Whether installing this release ends every worker session.
   *
   * `true` changes the terminal-host protocol, which swaps the API and the
   * engine together. `false` does not. `null` means the Hive could not tell —
   * the host did not answer, or the offer carries no readable protocol, and
   * the card must say so rather than fall back to the reassuring sentence.
   */
  carries_protocol_change: boolean | null;
  /** Commits this working copy is ahead of the released tag; null when uncountable. */
  commits_ahead_of_release: number | null;
  downloaded_version: string | null;
  /** What the install unit last reported, so a failure is visible. */
  apply_state: "installing" | "installed" | "failed" | "refused" | null;
  /** Why it refused, when it did. */
  apply_reason: string | null;
  /** Which step it failed at, in the operator's vocabulary. */
  apply_step: string | null;
  /** What the failing bundle itself said, one bounded line. */
  apply_detail: string | null;
  /** What the failure left behind, read back rather than assumed. */
  apply_changed: "nothing" | "partial" | "unknown" | null;
};

export async function fetchReleaseStatus(operatorToken: string): Promise<ReleaseStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/release");
  return response.json() as Promise<ReleaseStatus>;
}

export async function setReleaseCheckMode(operatorToken: string, mode: "off" | "daily"): Promise<ReleaseStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/release", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ mode }),
  });
  return response.json() as Promise<ReleaseStatus>;
}

export async function checkForRelease(operatorToken: string): Promise<ReleaseStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/release/check", { method: "POST" });
  return response.json() as Promise<ReleaseStatus>;
}

export async function downloadRelease(operatorToken: string): Promise<ReleaseStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/release/download", { method: "POST" });
  return response.json() as Promise<ReleaseStatus>;
}

export async function applyRelease(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/runtime/release/apply", { method: "POST" });
}

export type ResourcePressure = "normal" | "advisory" | "critical" | "unavailable";
export type RuntimeResources = {
  sampled_at: number;
  policy: { mode: "observe_only"; advisory_percent: number; critical_percent: number };
  api: ProcessResources & { pressure: ResourcePressure };
  terminal_host: ProcessResources & { pressure: ResourcePressure };
  machine?: MachineResources;
};
export type MachineResources = {
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
export type FederationStewardAssistState = "pending" | "accepted" | "declined";
export type FederationStewardAssistRequest = {
  id: string;
  apiary_id: string;
  source_hive_id: string;
  target_hive_id: string;
  message: string;
  state: FederationStewardAssistState;
  created_at: number;
  resolved_at: number | null;
};
export type FederationStewardAssistCommand = {
  id: string;
  apiary_id: string;
  action: { kind: "request"; target_hive_id: string; message: string }
    | { kind: "respond"; request_id: string; decision: FederationStewardAssistState };
  created_at: number;
};
export type FederationStewardAssistOutboxEntry = {
  command: FederationStewardAssistCommand;
  state: "queued" | "applied" | "rejected";
  attempt_count: number;
  last_attempt_at: number | null;
  receipt: {
    command_id: string;
    outcome: "applied" | "rejected";
    stewardship_id: string | null;
    request: FederationStewardAssistRequest | null;
    processed_at: number;
  } | null;
};
export type FederationStewardAssistLocalState = {
  incoming: FederationStewardAssistRequest[];
  sent?: FederationStewardAssistRequest[];
  outbox: FederationStewardAssistOutboxEntry[];
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
  /** Where it went, if anywhere. Null means it is on this Hive and nowhere else. */
  github_issue_url?: string | null;
};
/** One question in an interview-shaped decision request. */
export type DecisionQuestion = {
  header: string;
  question: string;
  options: string[];
  multi_select?: boolean;
};

export type DecisionRequest = {
  id: string;
  hive_id: string;
  requesting_worker_id: string;
  task_id: string | null;
  kind: "input" | "approval" | "credentials" | "conflict" | "help";
  urgency: "normal" | "time_sensitive";
  title: string;
  /** What the operator is deciding, in one or two sentences. */
  summary?: string;
  reason: string;
  risk: string;
  evidence: string;
  suggested_action: string;
  allowed_actions: string[];
  /**
   * The one command this request asks to be allowed to run.
   *
   * Present means one of the buttons grants permission to run it. The operator
   * must be able to READ it before pressing that button — a grant you cannot
   * read is worse than no grant mechanism at all.
   */
  requested_command?: string | null;
  /** Non-empty makes this an interview: answered by answering, not by a button. */
  questions?: DecisionQuestion[];
  deadline: number | null;
  state: "pending" | "resolved";
  resolution_action: string | null;
  /** The operator's answers, keyed by question header. Empty for a ruling. */
  resolution_answers?: Record<string, string[]>;
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

export async function fetchNotificationSettings(operatorToken: string, signal?: AbortSignal): Promise<NotificationSettings> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/notifications/settings", { signal });
  return response.json() as Promise<NotificationSettings>;
}

/**
 * Tells the Hive the operator is looking at Needs you right now.
 *
 * The watermark this advances is what stops every push notification firing for
 * the standing queue the moment they step away. So it must be called while the
 * queue is genuinely ON SCREEN — never from a background poll, which would mark
 * work seen that nobody read and silence the queue for good.
 *
 * Failure is ignored on purpose. Not recording a look means at worst one extra
 * notification; surfacing an error here would put a banner over the queue they
 * came to read.
 */
export async function recordAttentionSeen(operatorToken: string): Promise<void> {
  try {
    await authenticatedFetch(operatorToken, "/api/v1/notifications/seen", { method: "POST" });
  } catch {
    // Nothing useful to tell them, and nothing broken from their side.
  }
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
export async function fetchTerminalHostStatus(operatorToken: string, signal?: AbortSignal): Promise<TerminalHostStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/terminal-host", signal ? { signal } : undefined);
  const payload = (await response.json()) as { type: "host_status"; status: TerminalHostStatus };
  return payload.status;
}

/** An outside tool connected to this Hive through MCP. */
export type Connection = {
  id: string;
  name: string;
  connected_at: number;
  last_seen_at: number;
};

export async function fetchConnections(operatorToken: string): Promise<Connection[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/connections");
  const payload = (await response.json()) as { connections: Connection[] };
  return Array.isArray(payload.connections) ? payload.connections : [];
}

export async function revokeConnection(operatorToken: string, id: string): Promise<void> {
  await authenticatedFetch(operatorToken, `/api/v1/connections/${id}`, { method: "DELETE" });
}

export async function fetchRuntimeResources(operatorToken: string, signal?: AbortSignal): Promise<RuntimeResources> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/resources", signal ? { signal } : undefined);
  return response.json() as Promise<RuntimeResources>;
}

export async function fetchHistoryDiagnostics(operatorToken: string, signal?: AbortSignal): Promise<HistoryDiagnostics | null> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/history/diagnostics", signal ? { signal } : undefined);
  const payload = (await response.json()) as { type: "history_diagnostics"; diagnostics: HistoryDiagnostics | null };
  return payload.diagnostics;
}

export async function fetchHive(operatorToken: string, signal?: AbortSignal): Promise<HiveIdentity> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/hive", { signal });
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

export async function fetchFederationStewardAssists(
  operatorToken: string,
  signal?: AbortSignal,
): Promise<FederationStewardAssistLocalState> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/steward/assists", { signal });
  return response.json() as Promise<FederationStewardAssistLocalState>;
}

export async function queueFederationStewardAssist(
  operatorToken: string,
  input: { target_hive_id: string; message: string },
): Promise<FederationStewardAssistOutboxEntry> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/apiary/steward/assists", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
  });
  return response.json() as Promise<FederationStewardAssistOutboxEntry>;
}

export async function respondFederationStewardAssist(
  operatorToken: string,
  requestId: string,
  decision: "accepted" | "declined",
): Promise<FederationStewardAssistOutboxEntry> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/apiary/steward/assists/${encodeURIComponent(requestId)}/response`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ decision }),
  });
  return response.json() as Promise<FederationStewardAssistOutboxEntry>;
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

export async function fetchSessions(operatorToken: string, signal?: AbortSignal): Promise<SessionSummary[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/terminal/sessions", { signal });
  const payload = (await response.json()) as SessionsResponse;
  return payload.sessions;
}

export async function fetchDecisions(operatorToken: string, signal?: AbortSignal): Promise<DecisionRequest[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/decisions", { signal });
  return response.json() as Promise<DecisionRequest[]>;
}

/** Which control the operator used, recorded so a disputed answer can be traced. */
export type DecisionSurface = "inbox_action" | "inbox_dismiss" | "inbox_interview";

export async function resolveDecision(
  operatorToken: string,
  decisionId: string,
  action: string,
  note = "",
  surface: DecisionSurface | "" = "",
): Promise<DecisionRequest> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/decisions/${encodeURIComponent(decisionId)}/resolution`,
    { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ action, note, surface }) },
  );
  return response.json() as Promise<DecisionRequest>;
}

/** Answers an interview. Every declared question must carry an answer. */
export async function answerDecision(
  operatorToken: string,
  decisionId: string,
  answers: Record<string, string[]>,
  note = "",
  surface: DecisionSurface | "" = "inbox_interview",
): Promise<DecisionRequest> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/decisions/${encodeURIComponent(decisionId)}/resolution`,
    { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ answers, note, surface }) },
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

/** Whether this Hive can file a report anywhere, so the UI never offers what it cannot do. */
export async function fetchGithubFeedbackReadiness(
  operatorToken: string,
): Promise<{ configured: boolean; repository: string | null }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/feedback/github");
  return response.json() as Promise<{ configured: boolean; repository: string | null }>;
}

/** Which GitHub account this Hive files as, if a person has connected one. */
export async function fetchGithubConnection(
  operatorToken: string,
): Promise<{ connected: boolean; lapsed: boolean; login: string | null }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/github/connection");
  return response.json() as Promise<{ connected: boolean; lapsed: boolean; login: string | null }>;
}

/**
 * Starts connecting a GitHub account and returns the code to type.
 *
 * The device code itself stays on the server. What comes back is only what a
 * person needs to see.
 */
export async function startGithubConnection(
  operatorToken: string,
): Promise<{ user_code: string; verification_uri: string; expires_in: number; interval: number }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/integrations/github/connect", {
    method: "POST",
  });
  return response.json() as Promise<{
    user_code: string; verification_uri: string; expires_in: number; interval: number;
  }>;
}

/**
 * Asks once whether they have finished authorising.
 *
 * "waiting" is the ordinary answer for as long as somebody is typing, and is
 * not an error — GitHub reports it as one and a caller that believes it gives
 * up on an authorisation that is still live.
 */
export async function claimGithubConnection(
  operatorToken: string,
): Promise<{ state: "waiting" | "connected" | "declined" | "expired"; login?: string; interval?: number }> {
  const response = await authenticatedFetch(
    operatorToken,
    "/api/v1/integrations/github/connect/claim",
    { method: "POST" },
  );
  return response.json() as Promise<{
    state: "waiting" | "connected" | "declined" | "expired"; login?: string; interval?: number;
  }>;
}

/** Disconnects, so reports go back to being filed anonymously. */
export async function disconnectGithub(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/integrations/github/connection", {
    method: "DELETE",
  });
}

/** Files a saved report as a GitHub issue and returns where it went. */
export async function fileDogfoodReportOnGithub(
  operatorToken: string,
  reportId: string,
): Promise<{ issue_url: string; created: boolean }> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/feedback/reports/${encodeURIComponent(reportId)}/github`,
    { method: "POST" },
  );
  return response.json() as Promise<{ issue_url: string; created: boolean }>;
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
  signal?: AbortSignal,
): Promise<DogfoodReport[]> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/feedback/reports?limit=${encodeURIComponent(String(limit))}`,
    { signal },
  );
  return response.json() as Promise<DogfoodReport[]>;
}

export type EvidenceTiming = { count: number; total_ms: number; max_ms: number };
export type BrowserEvidenceHour = {
  capture_id: string;
  build: string;
  hour: number;
  revision: number;
  long_task: EvidenceTiming;
  interaction: EvidenceTiming;
  route: EvidenceTiming;
  terminal_render: EvidenceTiming;
  terminal_reconnect: EvidenceTiming;
};

export async function fetchBrowserEvidence(operatorToken: string, signal: AbortSignal): Promise<BrowserEvidenceHour[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/diagnostics/browser-evidence?limit=100", { signal });
  return response.json() as Promise<BrowserEvidenceHour[]>;
}

export async function recordBrowserEvidence(
  operatorToken: string, evidence: BrowserEvidenceHour, signal: AbortSignal,
): Promise<{ updated: boolean; pruned: number }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/diagnostics/browser-evidence", {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify(evidence), signal,
  });
  return response.json() as Promise<{ updated: boolean; pruned: number }>;
}

export async function downloadDogfoodScreenshot(
  operatorToken: string,
  attachmentName: string,
  signal?: AbortSignal,
): Promise<Blob> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/feedback/attachments/${encodeURIComponent(attachmentName)}`,
    { signal },
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

export async function fetchDevelopmentRuntime(operatorToken: string, signal?: AbortSignal): Promise<DevelopmentRuntime> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/development", { signal });
  return response.json() as Promise<DevelopmentRuntime>;
}

export async function requestDevelopmentReload(operatorToken: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/runtime/development/reload", { method: "POST" });
}

/**
 * Restarts EVERY live worker session, whatever provider release it is on.
 *
 * The lever for staleness nothing announces: a worker caches its MCP tool list
 * at connect, so a change to the agent tool surface reaches nobody until the
 * session reconnects — while the engine card correctly reports the engine as
 * current, because it is. restartSupersededWorkers cannot do this; it only
 * touches sessions whose provider release has moved on.
 *
 * This ends every live session. It is never automatic.
 */
/**
 * How many live sessions can call what this build serves.
 *
 * `unknown` is its own number and must never be folded into `current`: a
 * session that has not asked THIS build for its tools is not known to be fine,
 * and the in-memory record is emptied by an API restart while the sessions
 * themselves survive it.
 */
export async function fetchToolSurfaceStatus(operatorToken: string, signal?: AbortSignal): Promise<{
  serving_revision: number;
  live_sessions: number;
  current: number;
  stale: number;
  unknown: number;
}> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/tool-surface", { signal });
  return response.json() as Promise<{
    serving_revision: number; live_sessions: number; current: number; stale: number; unknown: number;
  }>;
}

export async function restartAllWorkers(operatorToken: string): Promise<{ restarted_workers: number }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/workers/restart", { method: "POST" });
  return response.json() as Promise<{ restarted_workers: number }>;
}

export async function restartSupersededWorkers(operatorToken: string): Promise<{ restarted_workers: number }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/providers/restart", { method: "POST" });
  return response.json() as Promise<{ restarted_workers: number }>;
}

export async function fetchProviderCapabilities(operatorToken: string, signal?: AbortSignal): Promise<ProviderCapabilities> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/providers", { signal });
  return response.json() as Promise<ProviderCapabilities>;
}

export async function fetchPresentationPreferences(
  operatorToken: string,
  deviceClass: PresentationDeviceClass,
  signal?: AbortSignal,
): Promise<PresentationPreferences> {
  const response = await authenticatedFetch(operatorToken, `/api/v1/preferences/presentation/${deviceClass}`, { signal });
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

export async function fetchQueenAutonomyPolicy(operatorToken: string, signal?: AbortSignal): Promise<QueenAutonomyPolicy> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/queen-policy", { signal });
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

export async function fetchQueenAutomationStatus(
  operatorToken: string,
  signal?: AbortSignal,
): Promise<QueenAutomationStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/queen-automation", { signal });
  return response.json() as Promise<QueenAutomationStatus>;
}

export async function fetchNotificationSubscriptionStatus(
  operatorToken: string,
  deviceId: string,
): Promise<{ registered: boolean }> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/notifications/subscriptions/${encodeURIComponent(deviceId)}`,
  );
  return response.json() as Promise<{ registered: boolean }>;
}

export async function fetchCoordinatorStatus(operatorToken: string, signal?: AbortSignal): Promise<CoordinatorStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/coordinator", { signal });
  return response.json() as Promise<CoordinatorStatus>;
}

export async function setQueenAutomationEnabled(
  operatorToken: string,
  enabled: boolean,
): Promise<QueenAutomationStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/queen-automation", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled }),
  });
  return response.json() as Promise<QueenAutomationStatus>;
}

export async function runQueenAutomation(operatorToken: string): Promise<QueenAutomationStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/orchestration/queen-automation/run", {
    method: "POST",
  });
  return response.json() as Promise<QueenAutomationStatus>;
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

/**
 * Replaces the operator token everywhere at once.
 *
 * Every browser session dies with it, including the one making the call, so the
 * caller must sign in again with the new value immediately or lock itself out
 * of its own rotation.
 */
export async function rotateOperatorToken(operatorToken: string, token: string): Promise<void> {
  await authenticatedFetch(operatorToken, "/api/v1/auth/token", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token }),
  });
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

export async function fetchHealth(signal?: AbortSignal): Promise<Health> {
  const response = await fetch("/health", { cache: "no-store", signal });
  if (!response.ok) throw new RuntimeRequestError(response.status, `Health returned ${response.status}`);
  return response.json() as Promise<Health>;
}

export type TunnelStatus = {
  /** Whether cloudflared is installed on the Hive's machine. */
  available: boolean;
  running: boolean;
  /** Whether the address has actually answered yet. Until it has, there is no QR. */
  serving: boolean;
  /** Why the last attempt gave up, when it did. */
  error: string | null;
  url: string | null;
  started_at: number | null;
  /** The address as an inline SVG QR code. The address only — never the token. */
  qr_svg: string | null;
};

export async function readTunnel(operatorToken: string, signal?: AbortSignal): Promise<TunnelStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/tunnel", { signal });
  return (await response.json()) as TunnelStatus;
}

export async function startTunnel(operatorToken: string): Promise<TunnelStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/tunnel/start", { method: "POST" });
  return (await response.json()) as TunnelStatus;
}

export async function stopTunnel(operatorToken: string): Promise<TunnelStatus> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/runtime/tunnel/stop", { method: "POST" });
  return (await response.json()) as TunnelStatus;
}

export type BlockedEscalation = {
  task_id: string;
  title: string;
  worker_name: string;
  workspace: string;
  blocked_for_seconds: number;
};
