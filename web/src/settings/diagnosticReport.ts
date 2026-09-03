import type {
  ControlRoomEvent,
  Health,
  HistoryDiagnostics,
  HiveIdentity,
  JiraReadiness,
  RuntimeResources,
  SessionSummary,
  TerminalHostStatus,
  Worker,
} from "../api";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import { readClientFailures } from "../feedback/clientDiagnostics";
import { readRoutePaints, routePaintSummary } from "../runtime/routePaint";
import { readBrowserPerformance } from "../runtime/browserPerformance";

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
  jiraReadiness?: JiraReadiness;
  jiraUnavailable?: boolean;
};

/**
 * The size the layout is actually deciding from, and what it decided.
 *
 * Added because a report arrived without it and could not be answered. The
 * operator sent a screenshot of a window 863 pixels wide showing the stacked
 * layout, saying "I am not using my phone at all" — and both halves were true.
 * At 1.5x display scaling that window is 575 CSS pixels, below the 680px
 * breakpoint, so the browser was right and the window looked wrong. Physical
 * size and CSS size are different numbers and only one of them is visible to a
 * person looking at their screen.
 *
 * Moving a window between monitors of different DPI changes this without
 * touching the window, which is what makes it intermittent and why it survives
 * a refresh and later clears on its own.
 *
 * Content-free, in keeping with the rest of this report: sizes and a media
 * query result, no text and nothing identifying.
 */
function viewportDiagnostics() {
  const stacked = (() => {
    try {
      return window.matchMedia?.("(max-width: 680px)").matches ?? null;
    } catch {
      return null;
    }
  })();
  return {
    css_width: window.innerWidth,
    css_height: window.innerHeight,
    device_pixel_ratio: window.devicePixelRatio,
    // What the window looks like to the person, as opposed to what the layout
    // is deciding from. The gap between these two is the whole diagnosis.
    physical_width: Math.round(window.innerWidth * window.devicePixelRatio),
    stacked_layout: stacked,
  };
}

export function buildSanitizedDiagnosticReport({ context, health, hiveIdentity, liveFeedState, recentEvents, runtime, sessions, workers, jiraReadiness, jiraUnavailable }: DiagnosticReportInput) {
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
      route_paint: routePaintSummary(readRoutePaints()),
      performance: readBrowserPerformance(),
      viewport: viewportDiagnostics(),
    },
    api: health ? { status: "healthy", version: health.version } : { status: "unavailable" },
    database: {
      status: hiveIdentity ? "reachable" : "unavailable",
      integrity: "not_checked",
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
      machine: runtime.resources.machine,
      worker_sessions: sessions.filter((session) => session.running).map(({ session_id, resources }) => ({ session_id, resources })),
    } : { status: runtime.loaded ? "unavailable" : "checking" },
    terminal_history: runtime.history ?? { status: runtime.loaded ? "unavailable" : "checking" },
    integrations: {
      jira: jiraUnavailable
        ? { status: "unavailable" }
        : jiraReadiness
          ? { status: jiraReadiness.connection, configured: jiraReadiness.configured }
          : { status: "checking" },
    },
    recent_state_transitions: recentEvents.slice(-16).map(({ sequence, kind, occurred_at }) => ({ sequence, kind, occurred_at })),
  };
}

export function serializeDiagnosticReport(input: DiagnosticReportInput) {
  return JSON.stringify(buildSanitizedDiagnosticReport(input), null, 2);
}
