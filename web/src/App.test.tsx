import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

vi.mock("./terminal/XtermSurface", () => ({ XtermSurface: class {} }));

import { App } from "./App";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";

afterEach(() => {
  cleanup();
  terminalWorkspace.logout();
  window.sessionStorage.clear();
  window.localStorage.clear();
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
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(ok({ status: "ok", version: "0.1.0" })));
  render(<App />);
  expect(await screen.findByText("Runtime 0.1.0")).toBeInTheDocument();
  expect(screen.getByText("Unlock this runtime to access tasks and workers.")).toBeInTheDocument();
});

test("makes runtime failure visible", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  render(<App />);
  expect(await screen.findByText("Runtime unavailable")).toBeInTheDocument();
});

test("keeps the operator token in the browser tab and reveals the control room", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [] }))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]));
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
  expect(window.sessionStorage.getItem("swarm-next.operator-token.v1")).toBe("secret");

  fireEvent.click(screen.getByRole("button", { name: "Lock" }));
  expect(window.sessionStorage.getItem("swarm-next.operator-token.v1")).toBeNull();
});

test("restores tasks and workers after a refresh", async () => {
  window.sessionStorage.setItem("swarm-next.operator-token.v1", "saved-secret");
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [{ session_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093", running: true }] }))
    .mockResolvedValueOnce(ok([{
      id: "worker-queen", name: "Queen", role: "queen", provider: "claude_code", workspace: "/workspace/queen", autostart: true, position: 0,
      active_session_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093", running: true, created_at: 1, updated_at: 1,
    }]))
    .mockResolvedValueOnce(ok([{ id: "task-1", title: "Stable reload", workspace: "/workspace", state: "active", assigned_session_id: "019fedfc-1c30-70e1-a5e2-9a3c94268093", created_at: 1, updated_at: 1 }]))
    .mockResolvedValueOnce(ok([]));
  vi.stubGlobal("fetch", fetch);

  render(<App />);

  expect(await screen.findByRole("heading", { name: "Stable reload" })).toBeInTheDocument();
  expect(screen.queryByLabelText("Operator token")).not.toBeInTheDocument();
  const sessionRequestHeaders = fetch.mock.calls[1]?.[1]?.headers as Headers;
  expect(sessionRequestHeaders.get("Authorization")).toBe("Bearer saved-secret");

  expect(screen.getByRole("option", { name: /Queen/ })).toBeInTheDocument();
});

test("restores the worker surface after a refresh", async () => {
  window.sessionStorage.setItem("swarm-next.operator-token.v1", "saved-secret");
  window.sessionStorage.setItem("swarm-next.surface.v1", "workers");
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [] }))
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

test("keyboard shortcuts switch workspaces but pause while editing a field", async () => {
  window.sessionStorage.setItem("swarm-next.operator-token.v1", "saved-secret");
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
    .mockResolvedValueOnce(ok(hiveIdentity()))
    .mockResolvedValueOnce(ok({ type: "sessions", sessions: [] }))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]))
    .mockResolvedValueOnce(ok([]));
  vi.stubGlobal("fetch", fetch);
  render(<App />);

  expect(await screen.findByRole("heading", { name: "Task board" })).toBeInTheDocument();
  fireEvent.keyDown(screen.getByRole("button", { name: "Tasks 0" }), { key: "4", altKey: true });
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Settings/ })).toHaveAttribute("aria-current", "page");

  fireEvent.keyDown(screen.getByRole("button", { name: /Settings/ }), { key: "3", altKey: true });
  expect(await screen.findByRole("heading", { name: "Worker terminal" })).toBeInTheDocument();

  const workerName = screen.getByLabelText("Add a named worker");
  workerName.focus();
  fireEvent.keyDown(workerName, { key: "4", altKey: true });
  expect(screen.getByRole("heading", { name: "Worker terminal" })).toBeInTheDocument();
});

test("removes a rejected saved token and returns to unlock", async () => {
  window.sessionStorage.setItem("swarm-next.operator-token.v1", "expired-secret");
  vi.stubGlobal(
    "fetch",
    vi.fn()
      .mockResolvedValueOnce(ok({ status: "ok", version: "0.1.0" }))
      .mockResolvedValue({ ok: false, status: 401, json: async () => ({ message: "expired" }) }),
  );

  render(<App />);

  expect(await screen.findByLabelText("Operator token")).toBeInTheDocument();
  expect(await screen.findByRole("alert")).toHaveTextContent("Runtime request returned 401: expired");
  expect(window.sessionStorage.getItem("swarm-next.operator-token.v1")).toBeNull();
});

test("creates a persisted task draft from the task board", async () => {
  const task = { id: "task-1", title: "Prove two workers", workspace: "/workspace", state: "draft", assigned_session_id: null, created_at: 1, updated_at: 1 };
  const responses = [
    ok({ status: "ok", version: "0.1.0" }),
    ok(hiveIdentity()),
    ok({ type: "sessions", sessions: [] }),
    ok([]),
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
    const response = responses.shift();
    if (!response) throw new Error(`Unexpected request: ${String(url)}`);
    return Promise.resolve(response);
  });
  vi.stubGlobal("fetch", fetch);
  render(<App />);
  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "secret" } });
  fireEvent.click(screen.getByRole("button", { name: "Unlock Swarm" }));

  fireEvent.change(await screen.findByLabelText("Task title"), { target: { value: task.title } });
  fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: task.workspace } });
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
