import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import JiraSettings from "./JiraSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("discovers a project, maps its workflow, and binds it to a repository worker", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.includes("/bindings") && method === "GET") return ok([]);
    if (url.includes("/projects?") && method === "GET") {
      return ok([{ id: "10001", key: "WEB", name: "Website Services" }]);
    }
    if (url.includes("/projects/10001/statuses")) {
      return ok([
        { id: "1", name: "To Do", category_key: "new", recommended_task_state: "ready" },
        { id: "3", name: "In Progress", category_key: "indeterminate", recommended_task_state: "active" },
        { id: "5", name: "Done", category_key: "done", recommended_task_state: "completed" },
      ]);
    }
    if (url.endsWith("/bindings") && method === "POST") {
      return new Response(JSON.stringify({ id: "binding-1" }), { status: 201, headers: { "Content-Type": "application/json" } });
    }
    if (url.includes("/bindings/binding-1/mappings") && method === "PUT") return ok([]);
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));

  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "ready", account_name: "Bea" }}
      unavailable={false}
      workers={[{
        id: "worker-1", hive_id: "hive-1", name: "Website", role: "worker", provider: "claude_code",
        workspace: "/projects/website", autostart: false, position: 1, active_session_id: null,
        created_at: 1, updated_at: 1, running: false, attention_state: "sleeping",
      }]}
    />,
  );

  fireEvent.change(screen.getByLabelText("Find a Jira project"), { target: { value: "web" } });
  fireEvent.click(await screen.findByRole("option", { name: "WEB Website Services" }));
  expect(await screen.findByText("In Progress")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Repository worker"), { target: { value: "worker-1" } });
  fireEvent.click(screen.getByRole("button", { name: "Connect project" }));

  expect(await screen.findByText("Website Services is ready for this Hive.")).toBeInTheDocument();
  const create = requests.find((request) => request.url.endsWith("/bindings") && request.method === "POST");
  expect(JSON.parse(create?.body ?? "{}")).toMatchObject({ project_id: "10001", default_worker_id: "worker-1" });
  const mapping = requests.find((request) => request.url.includes("/mappings") && request.method === "PUT");
  expect(JSON.parse(mapping?.body ?? "{}").mappings).toEqual([
    { jira_status_id: "1", jira_status_name: "To Do", task_state: "ready" },
    { jira_status_id: "3", jira_status_name: "In Progress", task_state: "active" },
    { jira_status_id: "5", jira_status_name: "Done", task_state: "completed" },
  ]);
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
