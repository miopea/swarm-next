import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

vi.mock("./terminal/XtermSurface", () => ({ XtermSurface: class {} }));
vi.mock("./terminal/TerminalView", () => ({ default: () => <div>Terminal ready</div> }));
vi.mock("./presence/PresenceController", () => ({
  deviceClass: () => "desktop",
  presenceDeviceId: () => "019fedfc-1c30-70e1-a5e2-9a3c94268093",
  PresenceController: class {
    start() {}
    stop() {}
    async enableLockDetection() { return false; }
  },
}));

vi.mock("./notifications/NotificationController", () => ({
  NotificationController: class {
    async start() {}
    stop() {}
    async enable() { return true; }
    async disable() {}
    async changePolicy() {}
    async test() {}
  },
}));
import { App } from "./App";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";

afterEach(() => {
  cleanup();
  terminalWorkspace.logout();
  window.sessionStorage.clear();
  window.localStorage.clear();
  window.history.replaceState({}, "", "/");
  delete document.documentElement.dataset.theme;
  document.documentElement.style.colorScheme = "";
  vi.unstubAllGlobals();
});

test("applies and remembers the selected color theme", async () => {
  window.localStorage.setItem("swarm-next.color-theme.v1", "dark");
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(ok({ status: "ok", version: "0.1.0" })));

  render(<App />);

  expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  fireEvent.click(screen.getByRole("button", { name: "Switch to light theme" }));
  expect(document.documentElement).toHaveAttribute("data-theme", "light");
  expect(window.localStorage.getItem("swarm-next.color-theme.v1")).toBe("light");
});

test("reports the connected runtime version", async () => {
  vi.stubGlobal("fetch", vi.fn((input: string | URL | Request) => Promise.resolve(
    String(input) === "/health" ? ok({ status: "ok", version: "0.1.0" }) : unauthorized(),
  )));
  render(<App />);
  expect(await screen.findByText("Runtime 0.1.0")).toBeInTheDocument();
  expect(screen.getByText("Unlock this runtime to access tasks and workers.")).toBeInTheDocument();
});

test("makes runtime failure visible", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  render(<App />);
  expect(await screen.findByText("Runtime unavailable")).toBeInTheDocument();
});

test("creates a durable browser session without storing the operator token", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(unauthorized())
    .mockResolvedValueOnce(ok({}))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [] }))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValue(ok({}));
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "secret" } });
  fireEvent.click(screen.getByRole("button", { name: "Unlock Swarm" }));

  expect(await screen.findByRole("heading", { name: "Task board" })).toBeInTheDocument();
  expect(await screen.findByRole("heading", { name: "What should the Hive take on next?" })).toBeInTheDocument();
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/tasks",
    expect.objectContaining({ cache: "no-store" }),
  );
  expect(screen.queryByDisplayValue("secret")).not.toBeInTheDocument();
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/auth/session",
    expect.objectContaining({
      method: "POST",
      credentials: "same-origin",
      headers: expect.any(Headers),
    }),
  );
  const sessionRequest = fetch.mock.calls.find(
    ([url, init]) => url === "/api/v1/auth/session" && (init as RequestInit | undefined)?.method === "POST",
  );
  expect((sessionRequest?.[1]?.headers as Headers).get("Authorization")).toBe("Bearer secret");
  expect(JSON.stringify({ local: { ...window.localStorage }, session: { ...window.sessionStorage } })).not.toContain("secret");

  // Locking is reachable rather than resident now: it left every header, where
  // it cost permanent room to offer something rarely wanted, and lives in the
  // command palette and Settings instead.
  fireEvent.click(screen.getByRole("button", { name: "Open quick navigation" }));
  fireEvent.click(screen.getByRole("option", { name: /Lock Swarm/ }));
  await waitFor(() => expect(screen.getByLabelText("Operator token")).toBeInTheDocument());
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/auth/session",
    expect.objectContaining({ method: "DELETE", credentials: "same-origin" }),
  );
});

test("restores tasks and workers after a refresh", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok({}))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [{ session_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093", running: true }] }))
    .mockResolvedValueOnce(ok([{
      id: "worker-queen", name: "Queen", role: "queen", provider: "claude_code", workspace: "/workspace/queen", autostart: true, position: 0,
      active_session_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093", running: true, created_at: 1, updated_at: 1,
    }]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([{ id: "task-1", title: "Stable reload", workspace: "/workspace", state: "active", assigned_session_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093", created_at: 1, updated_at: 1 }]))
    .mockResolvedValueOnce(ok([]));
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByRole("heading", { name: "Stable reload" })).toBeInTheDocument();
  expect(screen.queryByLabelText("Operator token")).not.toBeInTheDocument();
  const sessionRequestHeaders = fetch.mock.calls[1]?.[1]?.headers as Headers;
  expect(sessionRequestHeaders.get("Authorization")).toBeNull();
  expect(fetch.mock.calls[1]?.[1]).toEqual(expect.objectContaining({ credentials: "same-origin" }));

  expect(screen.getByRole("button", { name: "Workers, 1 active of 1" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Settings 3" })).not.toBeInTheDocument();
});

test("gives a Keeper a first-class Apiary control-room surface", async () => {
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok({
      operator: { id: "operator-1", display_name: "Bea" },
      hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: "apiary-1" },
      apiary_context: { mode: "federated", apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" }, local_role: "keeper" },
    }));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    if (url === "/api/v1/integrations/jira/bindings") return Promise.resolve(ok([]));
    if (url === "/api/v1/apiary/join-links") return Promise.resolve(ok([]));
    if (url === "/api/v1/integrations/jira/readiness") return Promise.resolve(ok({ configured: false, connection: "not_connected", account_name: null }));
    if (url === "/api/v1/integrations/email/readiness") return Promise.resolve(ok({ configured: false, connection: "not_connected", account_name: null }));
    if (url.includes("/api/v1/feedback/reports")) return Promise.resolve(ok([]));
    if (url === "/api/v1/terminal-host") return Promise.resolve(ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } }));
    if (url === "/api/v1/runtime/development") return Promise.resolve(ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false }));
    if (url === "/api/v1/runtime/resources") return Promise.resolve(ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 268435456, critical_bytes: 536870912 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } }));
    if (url === "/api/v1/terminal/history/diagnostics") return Promise.resolve(ok({ type: "history_diagnostics", diagnostics: null }));
    if (url.endsWith("/apiary/members")) return Promise.resolve(ok([{ hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true }]));
    if (url.endsWith("/apiary/jira-projects") || url.endsWith("/apiary/shared-work") || url.endsWith("/apiary/stewardships") || url.endsWith("/apiary/steward-task-audit")) return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/orchestration/queen-policy")) return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url.includes("/api/v1/orchestration/coordinator")) return Promise.resolve(ok({ completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0, stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0, last_action_at: null, automatic_start_admission: "allowed", automatic_start_batch_limit: 1, held: [] }));
    if (url.includes("/api/v1/providers")) return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url.includes("/api/v1/preferences/presentation/desktop")) return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    if (url.includes("/api/v1/integrations/jira/task-links")) return Promise.resolve(ok([]));
    return Promise.resolve(ok({ policy: "important_only", subscription_count: 0 }));
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  const apiary = await screen.findByRole("button", { name: /^Apiary/ });
  fireEvent.click(apiary);

  expect(await screen.findByRole("heading", { name: "Grand Garden" })).toBeInTheDocument();
  expect(apiary).toHaveAttribute("aria-current", "page");
  expect(screen.getByText("Registration, not live presence")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Manage Apiary" }));
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(window.location.hash).toBe("#settings-connections");
  expect(within(await screen.findByRole("navigation", { name: "Settings sections" })).getByRole("button", { name: "Connections" })).toHaveAttribute("aria-current", "location");

  cleanup();
  window.sessionStorage.clear();
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(within(await screen.findByRole("navigation", { name: "Settings sections" })).getByRole("button", { name: "Connections" })).toHaveAttribute("aria-current", "location");
  expect(window.location.hash).toBe("#settings-connections");
});

test("gives a Member Hive a first-class Apiary membership surface", async () => {
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok({
      operator: { id: "operator-2", display_name: "Cora" },
      hive: { id: "hive-2", name: "Clover Hive", operator_id: "operator-2", apiary_id: "apiary-1" },
      apiary_context: { mode: "federated", apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" }, local_role: "member" },
    }));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    if (url.endsWith("/apiary/members")) return Promise.resolve(ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: false },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true },
    ]));
    if (url.endsWith("/apiary/shared-work")) return Promise.resolve(ok([]));
    if (url.endsWith("/apiary/sync-health")) return Promise.resolve(ok({ condition: "idle", last_attempt_at: null, last_success_at: null, consecutive_failures: 0, next_attempt_at: null }));
    if (url.endsWith("/apiary/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/apiary/tasks/local-executions")) return Promise.resolve(ok([]));
    if (url.endsWith("/apiary/task-sync-status")) return Promise.resolve(ok({ cursor: 0, task_count: 0, last_applied_at: null }));
    if (url.endsWith("/apiary/task-outbox")) return Promise.resolve(ok([]));
    if (url.endsWith("/apiary/task-outbox-status")) return Promise.resolve(ok({ queued_count: 0, conflict_count: 0, rejected_count: 0, last_attempt_at: null }));
    if (url.endsWith("/apiary/catalog-readiness")) return Promise.resolve(ok({ acknowledgement: null, jira_connection: "ready", projects: [], blockers: ["catalog_missing"] }));
    if (url.endsWith("/apiary/steward/assists")) return Promise.resolve(ok({
      incoming: [{
        id: "019fedfc-1c30-70e1-a5e2-9a3c94268094",
        apiary_id: "apiary-1",
        source_hive_id: "hive-1",
        target_hive_id: "hive-2",
        message: "I can help review the release decision.",
        state: "pending",
        created_at: 100,
        resolved_at: null,
      }],
      sent: [],
      outbox: [],
    }));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/orchestration/queen-policy")) return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url.includes("/api/v1/providers")) return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url.includes("/api/v1/preferences/presentation/desktop")) return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    if (url.includes("/api/v1/integrations/jira/task-links")) return Promise.resolve(ok([]));
    return Promise.resolve(ok({ policy: "important_only", subscription_count: 0 }));
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  const needsYou = await screen.findByRole("button", { name: /^Needs you 1/ });
  // A queue holding work must not look like an empty one. The operator: "I
  // sometimes don't realize I have something pending which is slowing the whole
  // system down" — and every item here is something only they can clear, so a
  // silent queue stalls the fleet at its one irreplaceable participant.
  expect(within(needsYou).getByText("1")).toHaveAttribute("data-waiting");
  // Not colour alone: WCAG 2.1 AA, and it has to survive greyscale and forced
  // colours. The state reaches a screen reader as words.
  expect(needsYou).toHaveAccessibleName(/waiting for you/);
  // The tab title is the only part of this that reaches an operator who is not
  // looking at the page. A browser notification cannot serve the case they
  // reported, because push is deliberately suppressed while they are AT the
  // Hive — and being at the Hive is exactly when they missed it.
  await waitFor(() => expect(document.title).toBe("(1) Swarm"));
  expect(screen.getByLabelText("1 pending help offer")).toBeInTheDocument();
  fireEvent.click(needsYou);
  expect(await screen.findByRole("heading", { name: "A trusted Steward offered help" })).toBeInTheDocument();
  expect(screen.getByText(/Nothing was sent to a worker or terminal/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Review in Apiary" }));
  expect(await screen.findByRole("heading", { name: "Member Hive" })).toBeInTheDocument();

  const apiary = await screen.findByRole("button", { name: /^Apiary/ });
  fireEvent.click(apiary);
  expect(await screen.findByRole("heading", { name: "Grand Garden" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Member Hive" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Manage membership" })).toBeInTheDocument();
});

test("recovers runtime status and saved authentication after an update handoff", async () => {
  let healthAttempts = 0;
  let sessionAttempts = 0;
  const fetch = vi.fn((input: string | URL | Request) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(healthAttempts++ === 0 ? badGateway() : ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(sessionAttempts++ === 0 ? badGateway() : ok({}));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    return Promise.resolve(ok({}));
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByText("Runtime 0.1.0")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByLabelText("Operator token")).not.toBeInTheDocument());
  // Both calls failed once and were retried: the first attempt of each is the
  // gateway error. Not an exact count — the app also polls health on an
  // interval, so pinning a total here fails whenever a poll lands inside the
  // test rather than after it, which is scheduling, not behaviour.
  expect(healthAttempts).toBeGreaterThanOrEqual(2);
  expect(sessionAttempts).toBeGreaterThanOrEqual(2);
});

test("restores the saved session after a rolling API interruption", async () => {
  // Answered by URL rather than by call order. Chaining `mockResolvedValueOnce`
  // assumes the app issues one fixed sequence of requests, so a change in
  // scheduling handed the wrong payload to the wrong call and failed a test
  // about session recovery for reasons that had nothing to do with it.
  let sessionAttempts = 0;
  const fetch = vi.fn((input: string | URL | Request) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(sessionAttempts++ === 0 ? badGateway() : ok({}));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    return Promise.resolve(ok({}));
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  await waitFor(() => expect(screen.queryByLabelText("Operator token")).not.toBeInTheDocument());
  expect(screen.getByRole("heading", { name: "Task board" })).toBeInTheDocument();
  // The saved session was rejected once by the interruption and retried, which
  // is the recovery this test is about.
  expect(sessionAttempts).toBeGreaterThanOrEqual(2);
});

test("restores the worker surface after a refresh", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const fetch = bootFetch();
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByRole("heading", { name: "Worker terminal" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Workers, 0 active of 0" })).toHaveAttribute("aria-current", "page");
  expect(screen.queryByRole("heading", { name: "Task board" })).not.toBeInTheDocument();
  expect(window.sessionStorage.getItem("swarm-next.surface.v1")).toBe("workers");
});

test("restores the last selected live worker after reload", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const queenSession = "019fedfc-1c30-70e1-a5e2-9a3c94268081";
  const daisySession = "019fedfc-1c30-70e1-a5e2-9a3c94268082";
  window.localStorage.setItem("swarm-next.active-session.v1", daisySession);
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok({}))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [
      { session_id: queenSession, running: true },
      { session_id: daisySession, running: true },
    ] }))
    .mockResolvedValueOnce(ok([
      { id: "queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code", workspace: "/queen", autostart: true, position: 0, active_session_id: queenSession, running: true, attention_state: "buzzing", created_at: 1, updated_at: 1 },
      { id: "daisy", hive_id: "hive-1", name: "Daisy", role: "worker", provider: "claude_code", workspace: "/daisy", autostart: false, position: 1, active_session_id: daisySession, running: true, attention_state: "with_operator", created_at: 1, updated_at: 1 },
    ]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]));
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByRole("heading", { name: "Daisy" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Daisy/ })).toHaveAttribute("aria-current", "page");
});

test("removes completed assignments from the live worker roster", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const workerSession = "019fedfc-1c30-70e1-a5e2-9a3c94268083";
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok({}))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [{ session_id: workerSession, running: true }] }))
    .mockResolvedValueOnce(ok([{
      id: "daisy", hive_id: "hive-1", name: "Daisy", role: "worker", provider: "claude_code", workspace: "/daisy",
      autostart: false, position: 1, active_session_id: workerSession, running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
    }]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([{
      id: "task-1", title: "Already shipped", workspace: "/daisy", state: "completed", assigned_session_id: workerSession,
      created_at: 1, updated_at: 2,
    }]))
    .mockResolvedValueOnce(ok([]));
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByRole("button", { name: /^Daisy/ })).toHaveTextContent("daisy · Ready for work");
  expect(screen.queryByText("Already shipped")).not.toBeInTheDocument();
});

test("filters sleeping workers and remembers the choice on this device", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const queenSession = "019fedfc-1c30-70e1-a5e2-9a3c94268094";
  const workers = [
    {
      id: "worker-queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code",
      workspace: "/workspace/queen", autostart: true, position: 0, active_session_id: queenSession,
      running: true, attention_state: "resting", created_at: 1, updated_at: 1,
    },
    {
      id: "worker-platform", hive_id: "hive-1", name: "Platform", role: "worker", provider: "claude_code",
      workspace: "/workspace/platform", autostart: false, position: 1, active_session_id: null,
      running: false, attention_state: "sleeping", created_at: 1, updated_at: 1,
    },
  ];
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [{ session_id: queenSession, running: true }] }));
    if (url === "/api/v1/workers") return Promise.resolve(ok(workers));
    if (url === "/api/v1/workspaces" || url === "/api/v1/tasks" || url === "/api/v1/decisions") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) {
      return new Promise((_, reject) => init?.signal?.addEventListener(
        "abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true },
      ));
    }
    return Promise.resolve(ok({}));
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);
  expect(await screen.findByRole("button", { name: /^Platform/ })).toBeInTheDocument();
  const visibility = screen.getByRole("group", { name: "Workers shown" });
  fireEvent.click(within(visibility).getByRole("button", { name: "Awake" }));

  expect(screen.queryByRole("button", { name: /^Platform/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Queen/ })).toBeInTheDocument();
  expect(window.localStorage.getItem("swarm-next.worker-visibility.v1")).toBe("awake");

  cleanup();
  render(<App />);
  expect(await screen.findByRole("button", { name: /^Queen/ })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Platform/ })).not.toBeInTheDocument();
  expect(within(screen.getByRole("group", { name: "Workers shown" })).getByRole("button", { name: "Awake" })).toHaveAttribute("aria-pressed", "true");
});

test("opens the mobile worker picker without raising the keyboard over it", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const queenSession = "019fedfc-1c30-70e1-a5e2-9a3c94268094";
  const workers = [
    {
      id: "worker-queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code",
      workspace: "/workspace/queen", autostart: true, position: 0, active_session_id: queenSession,
      running: true, attention_state: "resting", created_at: 1, updated_at: 1,
    },
    {
      id: "worker-platform", hive_id: "hive-1", name: "Platform API", role: "worker", provider: "claude_code",
      workspace: "/workspace/rcg-platform-api", autostart: false, position: 1, active_session_id: null,
      running: false, attention_state: "sleeping", created_at: 1, updated_at: 1,
    },
    {
      id: "worker-sculpt", hive_id: "hive-1", name: "Sculpt Studio", role: "worker", provider: "claude_code",
      workspace: "/workspace/sculpt-studio", autostart: false, position: 2, active_session_id: null,
      running: false, attention_state: "sleeping", created_at: 1, updated_at: 1,
    },
  ];
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [{ session_id: queenSession, running: true }] }));
    if (url === "/api/v1/workers") return Promise.resolve(ok(workers));
    if (url === "/api/v1/workspaces" || url === "/api/v1/tasks" || url === "/api/v1/decisions") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) {
      return new Promise((_, reject) => init?.signal?.addEventListener(
        "abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true },
      ));
    }
    return Promise.resolve(ok({}));
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Switch worker, current Queen" }));
  const dialog = screen.getByRole("dialog", { name: "Where do you want to work?" });

  // The picker carried a search field that took focus on open, so asking to see
  // the roster covered it with a keyboard. There is no field on the phone now,
  // and nothing here focuses a text control.
  expect(within(dialog).queryByRole("searchbox")).not.toBeInTheDocument();
  expect(document.activeElement?.tagName).not.toBe("INPUT");
  expect(document.activeElement?.tagName).not.toBe("TEXTAREA");

  // The whole roster is reachable by scrolling instead.
  expect(within(dialog).getByRole("button", { name: /Platform API/ })).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: /Sculpt Studio/ })).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: /^Queen/ })).toBeInTheDocument();

  // Narrowing it is the Awake toggle's job, which needs no typing.
  fireEvent.click(within(dialog).getByRole("button", { name: "Awake" }));
  expect(within(dialog).getByRole("button", { name: /^Queen/ })).toBeInTheDocument();
  expect(within(dialog).queryByRole("button", { name: /Sculpt Studio/ })).not.toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "All" }));
  expect(within(dialog).getByRole("button", { name: /Sculpt Studio/ })).toBeInTheDocument();
});

test("switching workers releases only the previously selected engagement", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const queenSession = "019fedfc-1c30-70e1-a5e2-9a3c94268091";
  const workerSession = "019fedfc-1c30-70e1-a5e2-9a3c94268092";
  const workers = [
    {
      id: "worker-queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code",
      workspace: "/workspace/queen", autostart: true, position: 0, active_session_id: queenSession,
      running: true, attention_state: "with_operator", engagement_expires_at: Math.floor(Date.now() / 1000) + 300, created_at: 1, updated_at: 1,
    },
    {
      id: "worker-daisy", hive_id: "hive-1", name: "Daisy", role: "worker", provider: "claude_code",
      workspace: "/workspace/daisy", autostart: false, position: 1, active_session_id: workerSession,
      running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
    },
  ];
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [
      { session_id: queenSession, running: true }, { session_id: workerSession, running: true },
    ] }));
    if (url === "/api/v1/workers") return Promise.resolve(ok(workers));
    if (url === "/api/v1/workspaces" || url === "/api/v1/tasks" || url === "/api/v1/decisions") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) {
      return new Promise((_, reject) => init?.signal?.addEventListener(
        "abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true },
      ));
    }
    if (url.includes(`/api/v1/terminal/sessions/${queenSession}/engagements/`)) {
      return Promise.resolve(ok({}));
    }
    if (url.includes("/api/v1/runtime/tunnel")) {
      return Promise.resolve(ok({ available: false, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null }));
    }
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^Daisy/ }));

  await waitFor(() => expect(fetch).toHaveBeenCalledWith(
    `/api/v1/terminal/sessions/${queenSession}/engagements/019fedfc-1c30-70e1-a5e2-9a3c94268093`,
    expect.objectContaining({ method: "DELETE", credentials: "same-origin" }),
  ));
  expect(screen.getByRole("button", { name: /^Daisy/ })).toHaveAttribute("aria-current", "page");
});

test("notification navigation overrides a previously saved surface", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  window.history.replaceState({}, "", "/?surface=decisions");
  const fetch = bootFetch();
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByRole("heading", { name: "Needs you" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Needs you 0" })).toHaveAttribute("aria-current", "page");
});

/**
 * The operator kept seeing "Queen needs you", opened her, and found nothing.
 * This fixture is exactly that state: a review finished reporting
 * needs_operator, with no pending request from Queen anywhere.
 *
 * It used to produce a card saying she had filed a request and stopped. It
 * should produce nothing, because there is nothing to resolve.
 */
test("does not invent a Queen request that was never filed", async () => {
  const queenSession = "019fedfc-1c30-70e1-a5e2-9a3c94268093";
  const queen = {
    id: "worker-queen",
    hive_id: "hive-1",
    name: "Queen",
    role: "queen",
    provider: "claude_code",
    workspace: "/workspace/queen",
    autostart: true,
    position: 0,
    active_session_id: queenSession,
    running: true,
    attention_state: "resting",
    created_at: 1,
    updated_at: 1,
  };
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [{ session_id: queenSession, running: true }] }));
    if (url === "/api/v1/workers") return Promise.resolve(ok([queen]));
    if (url === "/api/v1/workspaces" || url === "/api/v1/tasks" || url === "/api/v1/decisions" || url === "/api/v1/integrations/jira/task-links") return Promise.resolve(ok([]));
    if (url === "/api/v1/orchestration/queen-policy") return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url === "/api/v1/orchestration/queen-automation") return Promise.resolve(ok({
      enabled: true,
      state: "completed",
      run_id: "run-1",
      trigger: "actionable_work",
      actionable_count: 1,
      attempts: 1,
      requested_at: 1,
      delivered_at: 2,
      finished_at: 3,
      outcome: "needs_operator",
      waiting_reason: null,
    }));
    if (url === "/api/v1/providers") return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url === "/api/v1/preferences/presentation/desktop") return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/runtime/tunnel")) {
      return Promise.resolve(ok({ available: false, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null }));
    }
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  // Nothing is waiting, so the queue says so rather than counting a phantom.
  const needsYou = await screen.findByRole("button", { name: "Needs you 0" });
  // And it carries no waiting state: an empty queue must not look like a full
  // one, in either direction.
  expect(within(needsYou).getByText("0")).not.toHaveAttribute("data-waiting");
  expect(needsYou).toHaveAccessibleName("Needs you 0");
  await waitFor(() => expect(document.title).toBe("Swarm"));
  fireEvent.click(needsYou);
  expect(await screen.findByRole("heading", { name: "Nothing needs your attention" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Queen needs you" })).not.toBeInTheDocument();
});
test("keyboard shortcuts switch workspaces but pause while editing a field", async () => {
  // Deliberately not bootFetch: this test navigates into Settings, which calls
  // endpoints this file does not model. Answering those with an empty object
  // is worse than not answering them, because Settings then renders against
  // shapes it cannot read.
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok({}))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [] }))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]));
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  expect(await screen.findByRole("heading", { name: "Task board" })).toBeInTheDocument();
  // The board heading renders before its controls do, so this waits rather
  // than assuming the two arrive together.
  fireEvent.click(await screen.findByRole("button", { name: "Write task" }));
  const taskTitle = screen.getByLabelText("Task title");
  taskTitle.focus();
  fireEvent.keyDown(taskTitle, { key: "4", altKey: true });
  expect(screen.getByRole("heading", { name: "Task board" })).toBeInTheDocument();
  fireEvent.keyDown(taskTitle, { key: "k", altKey: true });
  expect(screen.queryByRole("dialog", { name: "Where would you like to go?" })).not.toBeInTheDocument();

  fireEvent.keyDown(screen.getByRole("button", { name: "Tasks 0" }), { key: "k", altKey: true });
  expect(screen.getByRole("dialog", { name: "Where would you like to go?" })).toBeInTheDocument();
  expect(screen.getByRole("option", { name: /Add worker Configure a repository worker/ })).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Where would you like to go?" })).not.toBeInTheDocument();

  // Settings and the worker terminal are lazy surfaces, so reaching them waits
  // on a dynamic import as well as a render. The default one-second budget is
  // a statement about machine speed, not about whether navigation works.
  fireEvent.keyDown(screen.getByRole("button", { name: "Tasks 0" }), { key: "4", altKey: true });
  expect(await screen.findByRole("heading", { name: "Settings" }, { timeout: 5_000 })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");

  fireEvent.keyDown(screen.getByRole("button", { name: "Settings" }), { key: "3", altKey: true });
  expect(await screen.findByRole("heading", { name: "Worker terminal" }, { timeout: 5_000 })).toBeInTheDocument();
});

test("quietly returns to unlock when the server has no trusted cookie", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn()
      .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
      .mockResolvedValue({ ok: false, status: 401, json: async () => ({ message: "expired" }) }),
  );

  render(<App />);

  expect(await screen.findByLabelText("Operator token")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("creates a persisted task draft from the task board", async () => {
  const task = { id: "task-1", title: "Prove two workers", workspace: "/workspace", state: "draft", assigned_worker_id: null, assigned_session_id: null, created_at: 1, updated_at: 1 };
  const worker = {
    id: "worker-1", name: "Budget Bee", role: "worker", provider: "claude_code", workspace: task.workspace,
    autostart: false, position: 1, active_session_id: null, running: false, created_at: 1, updated_at: 1,
  };
  const responses = [
    unauthorized(),
    ok({}),
    ok(hiveIdentity()),
    ok({ type: "sessions", sessions: [] }),
    ok([worker]),
    ok([{ name: "workspace", path: task.workspace, kind: "repository", configured_worker_id: worker.id }]),
    ok([]),
    ok([]),
    ok([]),
    ok(task),
    ok({ ...task, assigned_worker_id: worker.id }),
  ];
  const fetch = vi.fn((url: string | URL | Request, init?: RequestInit) => {
    if (String(url).includes("/api/v1/control-room/events")) {
      return new Promise((_, reject) => {
        init?.signal?.addEventListener(
          "abort",
          () => reject(new DOMException("Aborted", "AbortError")),
          { once: true },
        );
      });
    }
    if (String(url).includes("/api/v1/orchestration/queen-policy")) {
      return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    }
    if (String(url).includes("/api/v1/orchestration/coordinator")) {
      return Promise.resolve(ok({ completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0, stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0, last_action_at: null, automatic_start_admission: "allowed", automatic_start_batch_limit: 1, held: [] }));
    }
    if (String(url).includes("/api/v1/orchestration/queen-automation")) {
      return Promise.resolve(ok({
        enabled: false,
        state: "idle",
        run_id: null,
        trigger: null,
        actionable_count: 0,
        attempts: 0,
        requested_at: null,
        delivered_at: null,
        finished_at: null,
        outcome: null,
        waiting_reason: null,
      }));
    }
    if (String(url).includes("/api/v1/providers")) {
      return Promise.resolve(ok({ claude_code: true, codex: false }));
    }
    if (String(url).includes("/api/v1/preferences/presentation/desktop")) {
      return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    }
    if (String(url).includes("/api/v1/integrations/jira/readiness")) {
      return Promise.resolve(ok({ configured: false, connection: "not_connected", account_name: null }));
    }
    if (String(url).endsWith("/api/v1/integrations/jira/bindings")) {
      return Promise.resolve(ok([]));
    }
    if (String(url).endsWith("/api/v1/integrations/email/task-links")) {
      return Promise.resolve(ok([]));
    }
    if (String(url).endsWith("/api/v1/tasks/removed")) {
      return Promise.resolve(ok([]));
    }
    // The runtime indicator polls these on its own schedule, so they must not
    // draw from the ordered list below — an extra poll would shift every
    // remaining response onto the wrong request.
    if (String(url) === "/health") {
      return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    }
    if (String(url).includes("/api/v1/runtime/terminal-host")) {
      return Promise.resolve(ok({ type: "host_status", status: { host_version: "0.1.0", draining: false } }));
    }
    if (String(url).endsWith("/integrations/email/awaiting-reply")) {
      return Promise.resolve(ok([]));
    }
    if (String(url).includes("/api/v1/runtime/development")) {
      return Promise.resolve(ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false }));
    }
    // Answered by name rather than from the queue below: that queue is consumed
    // in call order, so an unnamed endpoint silently takes another request's
    // response.
    //
    // The what's-new notes are exactly that hazard realised: adding the request
    // to App shifted every queued response by one and this test failed on a
    // heading four calls away from the cause.
    if (String(url).includes("/api/v1/runtime/release/notes")) {
      return Promise.resolve(ok({ running_version: "0.1.0", releases: [] }));
    }
    if (String(url).includes("/api/v1/preferences/start-surface")) {
      return Promise.resolve(ok({ start_surface: "tasks" }));
    }
    // The rail reads this on every surface to say whether the Hive is on the
    // internet. Named here for the same reason as the two above: the queue is
    // consumed in call order.
    if (String(url).includes("/api/v1/runtime/tunnel")) {
      return Promise.resolve(ok({ available: false, running: false, serving: false, error: null, url: null, started_at: null, qr_svg: null }));
    }
    const response = responses.shift();
    if (!response) throw new Error(`Unexpected request: ${String(url)}`);
    return Promise.resolve(response);
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);
  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "secret" } });
  fireEvent.click(screen.getByRole("button", { name: "Unlock Swarm" }));

  fireEvent.click(await screen.findByRole("button", { name: "Write task" }));
  fireEvent.change(await screen.findByLabelText("Task title"), { target: { value: task.title } });
  fireEvent.change(screen.getByLabelText("Who should handle this?"), { target: { value: worker.id } });
  fireEvent.click(screen.getByRole("button", { name: "Create draft" }));

  expect(await screen.findByRole("heading", { name: task.title })).toBeInTheDocument();
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/tasks",
    expect.objectContaining({ method: "POST", body: JSON.stringify({ title: task.title, description: "", priority: "normal", workspace: task.workspace }) }),
  );
  expect(fetch).toHaveBeenCalledWith(
    `/api/v1/tasks/${task.id}/assignment`,
    expect.objectContaining({ method: "PUT", body: JSON.stringify({ worker_id: worker.id }) }),
  );
});

test("waking a task worker assigns the stable worker rather than its new session", async () => {
  const sessionId = "019fedfc-1c30-70e1-a5e2-9a3c94268123";
  const worker = {
    id: "worker-daisy", hive_id: "hive-1", name: "Daisy", role: "worker", provider: "claude_code", workspace: "/daisy",
    autostart: false, position: 1, active_session_id: null, running: false, attention_state: "sleeping", created_at: 1, updated_at: 1,
  };
  const task = {
    id: "task-wake", title: "Resume durable work", workspace: worker.workspace, state: "ready",
    assigned_worker_id: worker.id, assigned_session_id: null, created_at: 1, updated_at: 1,
  };
  const runningWorker = { ...worker, active_session_id: sessionId, running: true, attention_state: "buzzing" };
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: method === "GET" && fetch.mock.calls.some(([called]) => String(called).endsWith(`/workers/${worker.id}/start`)) ? [{ session_id: sessionId, running: true }] : [] }));
    if (url === "/api/v1/workers") return Promise.resolve(ok(fetch.mock.calls.some(([called]) => String(called).endsWith(`/workers/${worker.id}/start`)) ? [runningWorker] : [worker]));
    if (url === "/api/v1/workspaces") return Promise.resolve(ok([{ name: "daisy", path: worker.workspace, kind: "repository", configured_worker_id: worker.id }]));
    if (url === "/api/v1/tasks") return Promise.resolve(ok([task]));
    if (url === "/api/v1/decisions") return Promise.resolve(ok([]));
    if (url.endsWith(`/workers/${worker.id}/start`)) return Promise.resolve(ok(runningWorker));
    if (url.endsWith(`/tasks/${task.id}/assignment`)) return Promise.resolve(ok({ ...task, assigned_session_id: sessionId }));
    if (url.endsWith(`/tasks/${task.id}/state`)) return Promise.resolve(ok({ ...task, state: "active", assigned_session_id: sessionId }));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/orchestration/queen-policy")) return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url.includes("/api/v1/providers")) return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url.includes("/api/v1/preferences/presentation/desktop")) return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    return Promise.resolve(ok({ policy: "important_only", subscription_count: 0 }));
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Open quick navigation" }));
  expect(screen.getByRole("option", { name: /Daisy Wake sleeping worker/ })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  fireEvent.click(await screen.findByRole("button", { name: "Wake Daisy" }));

  await waitFor(() => expect(fetch).toHaveBeenCalledWith(
    `/api/v1/tasks/${task.id}/assignment`,
    expect.objectContaining({ method: "PUT", body: JSON.stringify({ worker_id: worker.id }) }),
  ));
});

function hiveIdentity() {
  return { operator: { id: "operator-1", display_name: "Operator" }, hive: { id: "hive-1", name: "My Hive", operator_id: "operator-1", apiary_id: null } };
}

function ok(payload: unknown) {
  return { ok: true, status: 200, json: async () => payload };
}

function unauthorized() {
  return { ok: false, status: 401, json: async () => ({ message: "not unlocked" }) };
}

function badGateway() {
  return { ok: false, status: 502, json: async () => ({}) };
}

/**
 * Answers the App's start-up requests by URL rather than by call order.
 *
 * Chaining `mockResolvedValueOnce` assumes the app issues one fixed sequence of
 * requests. A change in scheduling then hands the wrong payload to the wrong
 * call, and tests fail for reasons that have nothing to do with what they
 * assert.
 */
test("the phone names the task its worker is carrying, without taking a row for it", async () => {
  // A phone could see which worker was selected and nothing about its work: the
  // context bar's task chip is hidden there, because a row of chips returns the
  // vertical space the phone layout reclaimed. The task takes the small line the
  // Hive indicator was using instead of adding one.
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const queenSession = "019fedfc-1c30-70e1-a5e2-9a3c94268094";
  const workers = [{
    id: "worker-queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code",
    workspace: "/workspace/queen", autostart: true, position: 0, active_session_id: queenSession,
    running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
  }];
  const tasks = [{
    id: "task-1", hive_id: "hive-1", title: "Render content blocks", description: "", operator_instruction: "",
    priority: "normal", workspace: "/workspace/queen", state: "active", assigned_worker_id: "worker-queen",
    assigned_session_id: queenSession, position: 0, created_at: 1, updated_at: 1,
  }];
  vi.stubGlobal("fetch", vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [{ session_id: queenSession, running: true }] }));
    if (url === "/api/v1/workers") return Promise.resolve(ok(workers));
    if (url === "/api/v1/tasks") return Promise.resolve(ok(tasks));
    if (url === "/api/v1/workspaces" || url === "/api/v1/decisions") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) {
      return new Promise((_, reject) => init?.signal?.addEventListener(
        "abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true },
      ));
    }
    return Promise.resolve(ok({}));
  }));

  render(<App />);

  const trigger = await screen.findByRole("button", { name: /Switch worker, current Queen/ });
  expect(trigger).toHaveAccessibleName("Switch worker, current Queen, carrying Render content blocks");
  expect(trigger).toHaveTextContent("Render content blocks");
  expect(trigger).toHaveTextContent("Queen");
});

test("a detached window shows its surface and nothing else", async () => {
  // Popping out produced a second copy of the whole app, which is
  // indistinguishable from another window of the same thing. A window opened
  // for one surface shows that surface, without navigation and without the
  // control to pop out again.
  window.history.replaceState({}, "", "/?surface=tasks&detached=1");
  vi.stubGlobal("fetch", bootFetch());

  render(<App />);

  expect(await screen.findByRole("heading", { name: "Task board" })).toBeInTheDocument();
  expect(screen.queryByRole("navigation", { name: "Swarm navigation" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Open .* in a new window/ })).not.toBeInTheDocument();
});

function bootFetch() {
  return vi.fn((input: string | URL | Request) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok(hiveIdentity()));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) {
      return Promise.resolve(ok([]));
    }
    return Promise.resolve(ok({}));
  });
}

test("a detached window keeps the controls belonging to what it shows", async () => {
  // The operator: "while secondary screens don't need to show the control areas
  // (needs you, tasks, etc) it still needs the filters, worker picker, etc.
  // This is the popped out worker panel with no way to change workers."
  //
  // So the rail survives detachment carrying the surface's own controls, and
  // loses the navigation between surfaces — which is what it was detached from.
  window.history.replaceState({}, "", "/?surface=workers&detached=1");
  const fetch = bootFetch();
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  await screen.findByRole("complementary", { name: "Workers controls" });
  // Navigation between surfaces is gone: this window is one surface.
  expect(screen.queryByRole("navigation", { name: "Primary" })).not.toBeInTheDocument();
});

/**
 * Held work has to disappear when the hold does.
 *
 * It was read once at sign-in, under a comment claiming it was polled. So a
 * card outlived the thing it described by an hour: every refusal in the
 * database was cleared, Queen was working, and "Needs you" still said she could
 * not review. An attention queue that shows resolved items is worse than an
 * empty one, because the next real item looks exactly like the stale one.
 */
test("stops showing held work once the coordinator is holding none", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let holding = true;
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok({
      operator: { id: "operator-1", display_name: "Bea" },
      hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null },
    }));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    if (url === "/api/v1/integrations/jira/bindings") return Promise.resolve(ok([]));
    if (url === "/api/v1/apiary/join-links") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/orchestration/queen-policy")) return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url.includes("/api/v1/orchestration/coordinator")) return Promise.resolve(ok({
      completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0,
      stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0,
      last_action_at: null, automatic_start_admission: "allowed", automatic_start_batch_limit: 1,
      held: holding
        ? [{ kind: "delivery_held_unsent_text", subject: "queen-review", worker_name: null, reason: "Queen's prompt holds text that was typed but never sent", first_observed_at: 1_787_402_241, observations: 172 }]
        : [],
    }));
    if (url.includes("/api/v1/providers")) return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url.includes("/api/v1/preferences/presentation/desktop")) return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    if (url.includes("/api/v1/integrations/jira/task-links")) return Promise.resolve(ok([]));
    if (url.includes("/api/v1/preferences/start-surface")) return Promise.resolve(ok({ start_surface: "decisions" }));
    return Promise.resolve(ok({ policy: "important_only", subscription_count: 0 }));
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  expect(await screen.findByText(/Queen cannot review/)).toBeInTheDocument();

  // Queen's prompt is cleared and the delivery lands; the coordinator holds
  // nothing. Nothing else in the page changes.
  holding = false;
  await vi.advanceTimersByTimeAsync(21_000);

  await waitFor(() => expect(screen.queryByText(/Queen cannot review/)).not.toBeInTheDocument());
  vi.useRealTimers();
});

/**
 * "It says (0) needs you items when that is on the page."
 *
 * Held work was rendered into the attention queue and counted by neither the
 * rail badge nor the tab, so the page showed a card above a badge reading zero.
 * A badge that disagrees with the page teaches the operator to stop believing
 * the badge, which is the only thing it has to do.
 */
test("counts held work in the Needs you badge", async () => {
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok({
      operator: { id: "operator-1", display_name: "Bea" },
      hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null },
    }));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    if (url === "/api/v1/integrations/jira/bindings") return Promise.resolve(ok([]));
    if (url === "/api/v1/apiary/join-links") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/orchestration/queen-policy")) return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url.includes("/api/v1/orchestration/coordinator")) return Promise.resolve(ok({
      completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0,
      stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0,
      last_action_at: null, automatic_start_admission: "allowed", automatic_start_batch_limit: 1,
      held: [{ kind: "delivery_held_open_prompt", subject: "task-brief:t1", worker_name: "Claude Shared Config", reason: "a briefing is waiting", first_observed_at: 1_787_402_241, observations: 4 }],
    }));
    if (url.includes("/api/v1/providers")) return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url.includes("/api/v1/preferences/presentation/desktop")) return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    if (url.includes("/api/v1/integrations/jira/task-links")) return Promise.resolve(ok([]));
    if (url.includes("/api/v1/preferences/start-surface")) return Promise.resolve(ok({ start_surface: "decisions" }));
    return Promise.resolve(ok({ policy: "important_only", subscription_count: 0 }));
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  expect(await screen.findByText(/has work waiting behind a prompt/)).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: /^Needs you/ })).toHaveTextContent("1"));
});

test("a blocked task old enough to escalate is counted, not just drawn", async () => {
  // THE BADGE MUST NOT DISAGREE WITH THE PAGE. The escalation was computed,
  // handed to the inbox as additionalPendingCount, and left out of the count —
  // so its card rendered while "Needs you" read 0. A badge that disagrees with
  // the page teaches the operator to stop believing the badge, which is the one
  // thing it has to do. It also silenced the push for it, because the watermark
  // only quiets sources the count knows about.
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
    if (url.endsWith("/integrations/email/awaiting-reply")) return Promise.resolve(ok([]));
    if (url === "/api/v1/hive") return Promise.resolve(ok({
      operator: { id: "operator-1", display_name: "Bea" },
      hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null },
    }));
    if (url === "/api/v1/terminal/sessions") return Promise.resolve(ok({ type: "sessions", sessions: [] }));
    if (["/api/v1/workers", "/api/v1/workspaces", "/api/v1/tasks", "/api/v1/decisions"].includes(url)) return Promise.resolve(ok([]));
    if (url === "/api/v1/integrations/jira/bindings") return Promise.resolve(ok([]));
    if (url === "/api/v1/apiary/join-links") return Promise.resolve(ok([]));
    if (url.includes("/api/v1/control-room/events")) return new Promise((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true }));
    if (url.includes("/api/v1/orchestration/queen-policy")) return Promise.resolve(ok({ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }));
    if (url.includes("/api/v1/orchestration/coordinator")) return Promise.resolve(ok({
      completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0,
      stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0,
      last_action_at: null, automatic_start_admission: "allowed", automatic_start_batch_limit: 1,
      held: [],
      blocked_escalations: [{ task_id: "019fedfc-1c30-70e1-a5e2-9a3c94268099", title: "A task nobody has come back to", worker_name: "Platform", workspace: "/workspace/platform", blocked_for_seconds: 50_400 }],
    }));
    if (url.includes("/api/v1/providers")) return Promise.resolve(ok({ claude_code: true, codex: false }));
    if (url.includes("/api/v1/preferences/presentation/desktop")) return Promise.resolve(ok({ device_class: "desktop", color_theme: "light", terminal_keys_visible: true, configured: true }));
    if (url.includes("/api/v1/integrations/jira/task-links")) return Promise.resolve(ok([]));
    if (url.includes("/api/v1/preferences/start-surface")) return Promise.resolve(ok({ start_surface: "decisions" }));
    return Promise.resolve(ok({ policy: "important_only", subscription_count: 0 }));
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  // The card is on the page...
  expect(await screen.findByText(/A task nobody has come back to/)).toBeInTheDocument();
  // ...and the badge agrees with it, which is the part that was missing.
  await waitFor(() => expect(screen.getByRole("button", { name: /^Needs you/ })).toHaveTextContent("1"));
});
