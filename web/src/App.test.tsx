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

  fireEvent.click(screen.getByRole("button", { name: "Lock" }));
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
  expect(window.location.hash).toBe("#settings-apiary");
  expect(within(await screen.findByRole("navigation", { name: "Settings sections" })).getByRole("button", { name: "Apiary" })).toHaveAttribute("aria-current", "location");

  cleanup();
  window.sessionStorage.clear();
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(within(await screen.findByRole("navigation", { name: "Settings sections" })).getByRole("button", { name: "Apiary" })).toHaveAttribute("aria-current", "location");
  expect(window.location.hash).toBe("#settings-apiary");
});

test("gives a Member Hive a first-class Apiary membership surface", async () => {
  const fetch = vi.fn((input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url === "/health") return Promise.resolve(ok({ status: "ok", version: "0.1.0" }));
    if (url === "/api/v1/auth/session") return Promise.resolve(ok({}));
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

  const needsYou = await screen.findByRole("button", { name: "Needs you 1" });
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
  expect(healthAttempts).toBe(2);
  expect(sessionAttempts).toBe(2);
});

test("restores the saved session after a rolling API interruption", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(badGateway())
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

  await waitFor(() => expect(screen.queryByLabelText("Operator token")).not.toBeInTheDocument());
  expect(screen.getByRole("heading", { name: "Task board" })).toBeInTheDocument();
  expect(fetch.mock.calls.filter(([url]) => url === "/api/v1/auth/session")).toHaveLength(2);
});

test("restores the worker surface after a refresh", async () => {
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
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

test("searches a large worker roster by worker or repository in the mobile switcher", async () => {
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
  fireEvent.change(within(dialog).getByRole("searchbox", { name: "Find a worker" }), { target: { value: "platform-api" } });

  expect(within(dialog).getByRole("button", { name: /Platform API/ })).toBeInTheDocument();
  expect(within(dialog).queryByRole("button", { name: /Sculpt Studio/ })).not.toBeInTheDocument();
  expect(within(dialog).queryByRole("button", { name: /^Queen/ })).not.toBeInTheDocument();

  fireEvent.click(within(dialog).getByRole("button", { name: "Awake" }));
  fireEvent.change(within(dialog).getByRole("searchbox", { name: "Find a worker" }), { target: { value: "sculpt" } });
  expect(within(dialog).getByText("That worker is sleeping")).toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "Show all workers" }));
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

  expect(await screen.findByRole("heading", { name: "Needs you" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Needs you 0" })).toHaveAttribute("aria-current", "page");
});

test("makes an operator-blocked Queen review first-class attention", async () => {
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
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  const needsYou = await screen.findByRole("button", { name: "Needs you 1" });
  fireEvent.click(needsYou);
  expect(await screen.findByRole("heading", { name: "Queen needs you" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "Needs you 1" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open Queen" }));
  expect(await screen.findByText("Terminal ready")).toBeInTheDocument();
});
test("keyboard shortcuts switch workspaces but pause while editing a field", async () => {
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
  fireEvent.click(screen.getByRole("button", { name: "Write task" }));
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

  fireEvent.keyDown(screen.getByRole("button", { name: "Tasks 0" }), { key: "4", altKey: true });
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");

  fireEvent.keyDown(screen.getByRole("button", { name: "Settings" }), { key: "3", altKey: true });
  expect(await screen.findByRole("heading", { name: "Worker terminal" })).toBeInTheDocument();
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
    ok({ status: "ok", version: "0.1.0" }),
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
