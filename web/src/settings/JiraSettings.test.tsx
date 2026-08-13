import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import JiraSettings from "./JiraSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("discovers a project, maps its workflow, and connects it as a shared Hive pool", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  let bound = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.includes("/bindings") && method === "GET") return ok(bound ? [{
      id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
      scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
    }] : []);
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
      bound = true;
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
    />,
  );

  fireEvent.change(screen.getByLabelText("Find a Jira project"), { target: { value: "web" } });
  fireEvent.click(await screen.findByRole("option", { name: "WEB Website Services" }));
  expect(await screen.findByText("In Progress")).toBeInTheDocument();
  expect(screen.getByText("Issues arrive unassigned. Assign or claim each one when its repository and worker are known.")).toBeInTheDocument();
  expect(screen.getByText(/Assignment is tracked separately from workflow/)).toBeInTheDocument();
  expect(screen.getAllByRole("option", { name: "In progress" }).length).toBeGreaterThan(0);
  fireEvent.click(screen.getByRole("button", { name: "Connect project" }));

  expect(await screen.findByText("Website Services is ready for this Hive.")).toBeInTheDocument();
  expect(screen.getByText("Shared with this Hive")).toBeInTheDocument();
  expect(screen.queryByRole("option", { name: "WEB Website Services" })).not.toBeInTheDocument();
  const create = requests.find((request) => request.url.endsWith("/bindings") && request.method === "POST");
  expect(JSON.parse(create?.body ?? "{}")).toMatchObject({ project_id: "10001" });
  expect(JSON.parse(create?.body ?? "{}")).not.toHaveProperty("default_worker_id");
  const mapping = requests.find((request) => request.url.includes("/mappings") && request.method === "PUT");
  expect(JSON.parse(mapping?.body ?? "{}").mappings).toEqual([
    { jira_status_id: "1", jira_status_name: "To Do", task_state: "ready" },
    { jira_status_id: "3", jira_status_name: "In Progress", task_state: "active" },
    { jira_status_id: "5", jira_status_name: "Done", task_state: "completed" },
  ]);
});

test("offers an operator-facing Atlassian connection instead of host-setting instructions", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.includes("/bindings")) return ok([]);
    if (url.endsWith("/auth/start") && init?.method === "POST") {
      return ok({ authorization_url: "https://auth.atlassian.test/authorize" });
    }
    throw new Error(`Unexpected request: ${init?.method ?? "GET"} ${url}`);
  }));
  const assign = vi.fn();

  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={{ configured: true, connection: "not_connected", account_name: null }}
      unavailable={false}
      onNavigate={assign}
    />,
  );

  expect(screen.getByText("Connect your Atlassian account to choose projects.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Connect with Atlassian" }));
  await waitFor(() => expect(assign).toHaveBeenCalledWith("https://auth.atlassian.test/authorize"));
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
