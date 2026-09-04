import { demoDecision, demoTasks, demoWorkers } from "./productFixtures";

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

export function hiveFixture(path: string): unknown | undefined {
  if (path === "/api/v1/diagnostics/browser-evidence") {
    const empty = { count: 0, total_ms: 0, max_ms: 0 };
    return ["1.4.1-dev-synthetic-a", "1.4.1-dev-synthetic-b"].map((build, index) => ({
      capture_id: `00000000-0000-0000-0000-00000000000${index + 1}`, build,
      hour: Math.floor(now / 3600) * 3600 - index * 3600, revision: 1,
      route: { count: 10, total_ms: 200 + index * 100, max_ms: 70 },
      long_task: empty, interaction: empty, terminal_render: empty, terminal_reconnect: empty,
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
      return demoTasks;
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
          load_average: [0.9, 0.8, 0.7],
          logical_cpus: 8,
          memory_pressure_avg10: 0,
          cpu_pressure_avg10: 0.4,
          io_pressure_avg10: 0,
          pressure: "normal",
        },
      };
    default:
      return undefined;
  }
}
