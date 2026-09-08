import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import JiraTaskIntake from "./JiraTaskIntake";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("claims explicitly selected unassigned open Jira work onto the task board", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bradford" });
    if (url.endsWith("/bindings")) return ok([{
      id: "binding-1", project_id: "10001", project_key: "WWD", project_name: "Website Development",
      scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
      auto_sync_assigned: true,
    }]);
    if (url.endsWith("/bindings/binding-1/issues")) return ok([
      { id: "20002", key: "WWD-43", summary: "Review mobile", description: "Check the Android PWA.", status_id: "3", status_name: "In Progress", assignee_account_id: null, assignee_name: null, updated_at: "now" },
    ]);
    if (url.endsWith("/bindings/binding-1/sync") && method === "POST") return ok([{ id: "task-1" }]);
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));
  const onImported = vi.fn().mockResolvedValue(undefined);

  render(<JiraTaskIntake operatorToken="operator-token" onImported={onImported} />);

  expect(await screen.findByText("Unassigned · open only")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "WWD Choose work" }));
  expect(await screen.findByRole("region", { name: "Choose Website Development work" })).toBeInTheDocument();
  expect(screen.getByText("Unassigned · open only")).toBeInTheDocument();
  expect(screen.getByText(/Swarm assigns it to Bradford in Jira/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Add 0 to this board" })).toBeDisabled();

  fireEvent.change(screen.getByLabelText("Find an issue"), { target: { value: "mobile" } });
  expect(screen.getByText("1 shown · 0 selected")).toBeInTheDocument();
  expect(screen.getByText("In Progress · Available to claim")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: /WWD-43/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add 1 to this board" }));

  await waitFor(() => expect(onImported).toHaveBeenCalled());
  const sync = requests.find((request) => request.url.endsWith("/sync"));
  expect(JSON.parse(sync?.body ?? "{}")).toEqual({ issue_ids: ["20002"] });
  expect(await screen.findByText("1 Jira issue added or refreshed on this board.")).toBeInTheDocument();
});

test("shows a retryable Jira error instead of an empty task source", async () => {
  let attempts = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (attempts++ === 0) return new Response("temporary gateway failure", { status: 502 });
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bradford" });
    if (url.endsWith("/bindings")) return ok([]);
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<JiraTaskIntake operatorToken="operator-token" onImported={vi.fn()} />);

  expect(await screen.findByRole("heading", { name: "Jira work could not be loaded" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("heading", { name: "No Jira projects are ready" })).toBeInTheDocument();
  expect(screen.getByText(/Settings → Integrations/)).toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}

test("a saved claim survives board-refresh failure and retries only the board read", async () => {
  let claims = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Demo" });
    if (url.endsWith("/bindings")) return ok([{ id: "fixture", project_key: "DEMO", project_name: "Fictional", workflow_mapped: true }]);
    if (url.endsWith("/issues")) return ok([{ id: "one", key: "DEMO-1", summary: "Fictional work", status_name: "Open" }]);
    if (url.endsWith("/sync")) { claims += 1; return ok([{ id: "saved-task" }]); }
    throw new Error(`Unexpected request: ${url}`);
  }));
  const onImported = vi.fn().mockRejectedValueOnce(new Error("board unavailable")).mockResolvedValue(undefined);
  render(<JiraTaskIntake operatorToken="operator-token" onImported={onImported} />);
  fireEvent.click(await screen.findByRole("button", { name: "DEMO Choose work" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: /DEMO-1/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add 1 to this board" }));
  expect(await screen.findByText(/Jira work was saved, but the board could not refresh/)).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: /DEMO-1/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Add 0 to this board" })).toBeDisabled();
  expect(screen.getByText("1 Jira issue added or refreshed on this board.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retry board refresh" }));
  await waitFor(() => expect(onImported).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Retry board refresh" })).not.toBeInTheDocument());
  expect(claims).toBe(1);
});

test("holds selection during a claim and retains it when the claim itself fails", async () => {
  let complete!: (response: Response) => void;
  const pending = new Promise<Response>((resolve) => { complete = resolve; });
  const claim = vi.fn().mockReturnValueOnce(pending).mockResolvedValue(ok([{ id: "saved-task" }]));
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready" });
    if (url.endsWith("/bindings")) return ok([{ id: "fixture", project_key: "DEMO", project_name: "Fictional", workflow_mapped: true }]);
    if (url.endsWith("/issues")) return ok([{ id: "one", key: "DEMO-1", summary: "Fictional work", status_name: "Open" }]);
    if (url.endsWith("/sync")) return claim();
    throw new Error(`Unexpected request: ${url}`);
  }));
  const onImported = vi.fn().mockResolvedValue(undefined);
  render(<JiraTaskIntake operatorToken="operator-token" onImported={onImported} />);
  fireEvent.click(await screen.findByRole("button", { name: "DEMO Choose work" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: /DEMO-1/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add 1 to this board" }));
  await waitFor(() => expect(claim).toHaveBeenCalledTimes(1));
  expect(screen.getByRole("checkbox", { name: /DEMO-1/ })).toBeDisabled();
  for (const name of ["Close", "Clear", "Select shown"]) expect(screen.getByRole("button", { name })).toBeDisabled();
  await act(async () => { complete(new Response("claim rejected", { status: 400 })); });
  await waitFor(() => expect(screen.getByRole("button", { name: "Add 1 to this board" })).toBeEnabled());
  expect(screen.getByRole("checkbox", { name: /DEMO-1/ })).toBeChecked();
  expect(onImported).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Retry board refresh" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Add 1 to this board" }));
  expect(await screen.findByText("1 Jira issue added or refreshed on this board.")).toBeInTheDocument();
  expect(claim).toHaveBeenCalledTimes(2);
  expect(onImported).toHaveBeenCalledTimes(1);
});
