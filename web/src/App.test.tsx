import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  expect(screen.getByRole("heading", { name: "Give the next worker a clear outcome" })).toBeInTheDocument();
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

  expect(screen.getByRole("button", { name: "Workers 1" })).toBeInTheDocument();
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
  expect(screen.getByRole("button", { name: "Workers 0" })).toHaveAttribute("aria-current", "page");
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

  expect(await screen.findByRole("button", { name: /^Daisy/ })).toHaveTextContent("Unassigned session");
  expect(screen.queryByText("Already shipped")).not.toBeInTheDocument();
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
  const taskTitle = screen.getByLabelText("Task title");
  taskTitle.focus();
  fireEvent.keyDown(taskTitle, { key: "4", altKey: true });
  expect(screen.getByRole("heading", { name: "Task board" })).toBeInTheDocument();
  fireEvent.keyDown(taskTitle, { key: "k", altKey: true });
  expect(screen.queryByRole("dialog", { name: "Where would you like to go?" })).not.toBeInTheDocument();

  fireEvent.keyDown(screen.getByRole("button", { name: "Tasks 0" }), { key: "k", altKey: true });
  expect(screen.getByRole("dialog", { name: "Where would you like to go?" })).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Where would you like to go?" })).not.toBeInTheDocument();

  fireEvent.keyDown(screen.getByRole("button", { name: "Tasks 0" }), { key: "4", altKey: true });
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Settings/ })).toHaveAttribute("aria-current", "page");

  fireEvent.keyDown(screen.getByRole("button", { name: /Settings/ }), { key: "3", altKey: true });
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
  const task = { id: "task-1", title: "Prove two workers", workspace: "/workspace", state: "draft", assigned_session_id: null, created_at: 1, updated_at: 1 };
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
    ok(task),
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
    const response = responses.shift();
    if (!response) throw new Error(`Unexpected request: ${String(url)}`);
    return Promise.resolve(response);
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);
  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "secret" } });
  fireEvent.click(screen.getByRole("button", { name: "Unlock Swarm" }));

  fireEvent.change(await screen.findByLabelText("Task title"), { target: { value: task.title } });
  fireEvent.click(screen.getByRole("button", { name: "Create draft" }));

  expect(await screen.findByRole("heading", { name: task.title })).toBeInTheDocument();
  expect(fetch).toHaveBeenLastCalledWith(
    "/api/v1/tasks",
    expect.objectContaining({ method: "POST", body: JSON.stringify({ title: task.title, description: "", priority: "normal", workspace: task.workspace }) }),
  );
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
