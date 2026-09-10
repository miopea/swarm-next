import { demoDecision, demoTasks, demoWorkers } from "./productFixtures";
import type { TaskActivityPage } from "../api";

/**
 * Enough of a Hive for the real App to mount against.
 *
 * WHY THIS EXISTS. A harness surface renders one component, which is right for
 * looking at a card while fixing it and useless as a product screenshot: the
 * picture is a panel on a beige field with no rail, no header and no way to
 * tell what application it belongs to. That mistake reached a public README.
 *
 * Extracting the shell out of App.tsx was the obvious alternative and was
 * measured first: 236 lines of JSX, thirty call sites and thirty-four
 * interpolated identifiers — a sixty-prop refactor of the NAVIGATION, risked on
 * the operator's live control room, to produce three pictures. Not worth it.
 *
 * So the App mounts unchanged and the NETWORK answers with fixtures. Nothing in
 * production learns that a harness exists, and the capture is the real shell
 * because it IS the real shell.
 */
const now = Math.floor(Date.now() / 1000);

export function hiveFixture(path: string, query = new URLSearchParams()): unknown | undefined {
  if (path === "/api/v1/decisions" && new URLSearchParams(window.location.search).get("decisionHistory") === "1") return [
    { ...demoDecision, id: "fixture-pending", title: "Fixture decision pending" },
    { ...demoDecision, id: "fixture-answered", title: "Fixture decision answered", state: "resolved", resolution_action: "Fixture answer", resolved_at: now },
    { ...demoDecision, id: "fixture-withdrawn", title: "Fixture decision withdrawn", state: "withdrawn", resolved_at: now },
    { ...demoDecision, id: "fixture-waiting", title: "Fixture decision awaiting explanation", clarification: { next_move: "requester" } },
  ];
  // An empty task/attention board is valid even while terminals are in use.
  if (new URLSearchParams(window.location.search).get("emptyWork") === "1"
    && ["/api/v1/tasks", "/api/v1/decisions"].includes(path)) return [];
  const activityTask = /^\/api\/v1\/tasks\/([^/]+)\/activity$/.exec(path);
  if (activityTask && new URLSearchParams(window.location.search).get("taskHistory") === "paged") {
    const before = Number(query.get("before") ?? 76);
    const limit = Math.min(100, Math.max(1, Number(query.get("limit") ?? 30)));
    const end = Math.min(75, before - 1);
    const start = Math.max(1, end - limit + 1);
    return {
      events: Array.from({ length: Math.max(0, end - start + 1) }, (_, index) => ({
        sequence: start + index, task_id: decodeURIComponent(activityTask[1]),
        kind: "details_updated", from_state: null, to_state: null,
        actor_kind: "worker", actor_id: "demo-worker",
        note: `Fictional handoff ${start + index}: verified the next step with Petal.`,
        occurred_at: now - (75 - start - index) * 60,
      })),
      truncated: start > 1,
    } satisfies TaskActivityPage;
  }
  if (activityTask) return {
    events: [
      { sequence: 1, task_id: decodeURIComponent(activityTask[1]), kind: "created",
        from_state: null, to_state: "draft", actor_kind: "operator", actor_id: null,
        note: "Fictional task created for UI verification.", occurred_at: now - 120 },
      { sequence: 2, task_id: decodeURIComponent(activityTask[1]), kind: "details_updated",
        from_state: null, to_state: null, actor_kind: "worker", actor_id: "demo-worker",
        note: "Fictional handoff recorded. No live task was changed.", occurred_at: now - 60 },
    ],
    truncated: false,
  } satisfies TaskActivityPage;
  if (path === "/api/v1/orchestration/coordinator" && new URLSearchParams(window.location.search).get("startHold") === "1") return {
    completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 1,
    stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0,
    last_action_at: null, automatic_start_admission: "deferred_advisory", automatic_start_batch_limit: 1,
    held: [{ kind: "wake_not_admitted", subject: "wake:fictional-task", worker_name: "Orchard API",
      reason: "Orchard API is waiting for the machine's memory pressure to ease before it can start.",
      first_observed_at: now, observations: 1 }],
  };
  const cpuOnlyPressure = new URLSearchParams(window.location.search).get("machinePressure") === "cpu-only";
  if (path === "/api/v1/runtime/queen-history") return {
    retention_days: 30, max_retained: 4096, retained_count: 0, records: [],
    review_returns: { retention_days: 30, max_retained: 4096, retained_count: 200, records: [
      { request_id: "fixture-return-a", returned_on_build: "1.6.0-dev-fictional-review", returned_at: now-120, answered_at: now-60 },
      { request_id: "fixture-return-b", returned_on_build: "1.6.0-dev-fictional-review", returned_at: now-90, answered_at: null },
    ] },
  };
  if (path === "/api/v1/diagnostics/browser-evidence") {
    const empty = { count: 0, total_ms: 0, max_ms: 0 };
    return ["1.4.1-dev-synthetic-a", "1.4.1-dev-synthetic-b"].map((build, index) => ({
      capture_id: `00000000-0000-0000-0000-00000000000${index + 1}`, build,
      hour: Math.floor(now / 3600) * 3600 - index * 3600, revision: 1,
      route: { count: 10, total_ms: 200 + index * 100, max_ms: 70 },
      long_task: empty, interaction: empty, terminal_render: empty, terminal_reconnect: empty,
      terminal_grant: empty, terminal_socket: empty, terminal_restore: empty,
    }));
  }
  if (path === "/api/v1/presence/night-watch") return { enabled: false, timezone: "America/New_York", start_minute: 1320, end_minute: 420 };
  // A terminal attachment asks for a grant at a per-session path, so it cannot
  // be an arm of the switch below. The grant is a fiction like everything else
  // here: FixtureWebSocket never dials the websocket_path it names.
  if (path.startsWith("/api/v1/terminal/sessions/") && path.endsWith("/attach-grants")) {
    return {
      grant: "harness-grant",
      protocol: "swarm-terminal.v4",
      websocket_path: "/api/v1/terminal/attach",
      expires_in_ms: 60_000,
    };
  }
  switch (path) {
    case "/health":
      return { status: "ok", version: "1.0.0", degraded: [] };
    // Answering this at all is what unlocks the app: App.tsx restores a browser
    // session before it will render anything but the token form.
    case "/api/v1/auth/session":
      return { authenticated: true };
    case "/api/v1/hive":
      return {
        operator: { id: "demo-operator", display_name: "You" },
        hive: { id: "demo-hive", name: "Orchard", operator_id: "demo-operator", apiary_id: null },
      };
    case "/api/v1/workers":
      return demoWorkers;
    case "/api/v1/workers/conversations":
      return { workers: new URLSearchParams(window.location.search).get("history") === "unknown"
        ? [{ worker_id: "demo-history-worker", name: "Petal", freshness: { state: "unknown", reason: "No readable conversation entry in this synthetic workspace" } }]
        : [] };
    case "/api/v1/tasks":
      return new URLSearchParams(window.location.search).get("dependencies") === "linked"
        ? demoTasks.map((task, index) => index === 3 ? { ...task, prerequisites: [{
          task_id: task.id, prerequisite_id: demoTasks[2].id, title: demoTasks[2].title,
          state: demoTasks[2].state, assigned_worker_id: demoTasks[2].assigned_worker_id,
          removed: false, reason: "The API task must finish before this view can consume its contract.", created_at: now,
        }] } : task)
        : demoTasks;
    case "/api/v1/decisions":
      // An EMPTY queue is a poor advertisement for the product's main screen:
      // the first capture showed "Nothing needs your attention", which is true
      // and says nothing about what Swarm does.
      return [demoDecision];
    case "/api/v1/terminal/sessions":
      // Enveloped, unlike the bare arrays around it. These two session ids are
      // the ones demoWorkers carries, so the roster shows running workers and
      // the selected one has a terminal to attach.
      return {
        type: "sessions",
        sessions: [
          { session_id: "session-queen", running: true },
          { session_id: "session-web", running: true },
        ],
      };
    case "/api/v1/workspaces":
      return ["/home/you/projects/orchard", "/home/you/projects/orchard-web"];
    case "/api/v1/runtime/resources":
      // Normal, so the header carries no pressure badge in a screenshot.
      return {
        daily_backup: (() => {
          const state = new URLSearchParams(window.location.search).get("backupState");
          return state === "failed" || state === "unavailable" ? { state }
            : state === "ready" ? { state, snapshot_day: "20260904" } : { state: "not_reported" };
        })(),
        sampled_at: now,
        policy: { mode: "observe_only", advisory_percent: 85, critical_percent: 95 },
        api: { resident_memory_bytes: 92 * 1024 * 1024, pressure: "normal" },
        terminal_host: { resident_memory_bytes: 48 * 1024 * 1024, pressure: "normal" },
        machine: {
          memory_total_bytes: 32 * 1024 ** 3,
          memory_available_bytes: 20 * 1024 ** 3,
          memory_used_percent: 37,
          swap_total_bytes: 8 * 1024 ** 3,
          swap_used_bytes: 0,
          swap_used_percent: 0,
          load_average: cpuOnlyPressure ? [18, 8, 4] : [0.9, 0.8, 0.7],
          logical_cpus: 8,
          memory_pressure_avg10: 0,
          cpu_pressure_avg10: cpuOnlyPressure ? 65 : 0.4,
          io_pressure_avg10: 0,
          memory_pressure: "normal",
          memory_stall_pressure: "normal",
          pressure: cpuOnlyPressure ? "critical" : "normal",
        },
      };
    default:
      return undefined;
  }
}
