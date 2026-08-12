import type {
  ControlRoomEvent,
  Health,
  HistoryDiagnostics,
  HiveIdentity,
  RuntimeResources,
  SessionSummary,
  TerminalHostStatus,
  Worker,
} from "../api";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import { readClientFailures } from "../feedback/clientDiagnostics";

export type RuntimeDiagnostics = {
  terminalHost?: TerminalHostStatus;
  history?: HistoryDiagnostics | null;
  resources?: RuntimeResources;
  loaded: boolean;
};

export type DiagnosticContext = {
  surface?: string;
  selectedSessionId?: string;
  expectation?: string;
  observation?: string;
};

type DiagnosticReportInput = {
  context?: DiagnosticContext;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  recentEvents: ControlRoomEvent[];
  runtime: RuntimeDiagnostics;
  sessions: SessionSummary[];
  workers: Worker[];
};

export function buildSanitizedDiagnosticReport({ context, health, hiveIdentity, liveFeedState, recentEvents, runtime, sessions, workers }: DiagnosticReportInput) {
  const launchFailures = workers.filter((worker) => Boolean(worker.runtime_error)).length;
  const expectation = context?.expectation?.trim();
  const observation = context?.observation?.trim();
  const operatorNote = expectation || observation ? {
    expectation: expectation || null,
    observation: observation || null,
    privacy: "operator supplied; review before copying",
  } : undefined;

  return {
    schema_version: 1,
    generated_at: new Date().toISOString(),
    correlation_id: globalThis.crypto?.randomUUID?.() ?? `local-${Date.now()}`,
    privacy: "automatic collection is content-free: no terminal output, task text, paths, credentials, worker names, or raw errors",
    context: context?.surface || context?.selectedSessionId ? {
      surface: context.surface ?? null,
      selected_session_id: context.selectedSessionId ?? null,
    } : undefined,
    operator_note: operatorNote,
    browser: {
      status: navigator.onLine ? "online" : "offline",
      visibility: document.visibilityState,
      live_updates: liveFeedState,
      recent_failures: readClientFailures(),
    },
    api: health ? { status: "healthy", version: health.version } : { status: "unavailable" },
    database: {
      status: hiveIdentity ? "healthy" : "unavailable",
      hive_id: hiveIdentity?.hive.id,
    },
    terminal_host: runtime.terminalHost
      ? { status: runtime.terminalHost.draining ? "draining" : "healthy", ...runtime.terminalHost }
      : { status: runtime.loaded ? "unavailable" : "checking" },
    provider: {
      status: launchFailures > 0 ? "degraded" : "healthy",
      configured_workers: workers.length,
      running_workers: workers.filter((worker) => worker.running).length,
      launch_failures: launchFailures,
      session_ids: sessions.map((session) => session.session_id),
    },
    runtime_resources: runtime.resources ? {
      policy: runtime.resources.policy,
      api: runtime.resources.api,
      terminal_host: runtime.resources.terminal_host,
    } : { status: runtime.loaded ? "unavailable" : "checking" },
    terminal_history: runtime.history ?? { status: runtime.loaded ? "unavailable" : "checking" },
    integrations: { status: "not_configured" },
    recent_state_transitions: recentEvents.slice(-16).map(({ sequence, kind, occurred_at }) => ({ sequence, kind, occurred_at })),
  };
}

export function serializeDiagnosticReport(input: DiagnosticReportInput) {
  return JSON.stringify(buildSanitizedDiagnosticReport(input), null, 2);
}
