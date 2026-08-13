import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import JiraTaskIntake from "./JiraTaskIntake";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("brings explicitly selected assigned open Jira work onto the task board", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.endsWith("/readiness")) return ok({ configured: true, connection: "ready", account_name: "Bradford" });
    if (url.endsWith("/bindings")) return ok([{
      id: "binding-1", project_id: "10001", project_key: "WWD", project_name: "Website Development",
      scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
    }]);
    if (url.endsWith("/bindings/binding-1/issues")) return ok([
      { id: "20001", key: "WWD-42", summary: "Polish launch", status_id: "1", status_name: "To Do", assignee_account_id: "a1", assignee_name: "Bradford", updated_at: "now" },
      { id: "20002", key: "WWD-43", summary: "Review mobile", status_id: "3", status_name: "In Progress", assignee_account_id: "a1", assignee_name: "Bradford", updated_at: "now" },
    ]);
    if (url.endsWith("/bindings/binding-1/sync") && method === "POST") return ok([{ id: "task-1" }]);
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));
  const onImported = vi.fn().mockResolvedValue(undefined);

  render(<JiraTaskIntake operatorToken="operator-token" onImported={onImported} />);

  expect(await screen.findByText("Open issues assigned to Bradford")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "WWD Choose work" }));
  expect(await screen.findByText("Assigned to Bradford · open only")).toBeInTheDocument();
  expect(screen.getByText(/Nothing is selected or imported/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Add 0 to this board" })).toBeDisabled();

  fireEvent.change(screen.getByLabelText("Find an issue"), { target: { value: "mobile" } });
  expect(screen.getByText("1 shown · 0 selected")).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: /WWD-42/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: /WWD-43/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add 1 to this board" }));

  await waitFor(() => expect(onImported).toHaveBeenCalled());
  const sync = requests.find((request) => request.url.endsWith("/sync"));
  expect(JSON.parse(sync?.body ?? "{}")).toEqual({ issue_ids: ["20002"] });
  expect(await screen.findByText("1 Jira issue added or refreshed on this board.")).toBeInTheDocument();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
