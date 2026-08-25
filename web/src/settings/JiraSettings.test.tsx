import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import JiraSettings from "./JiraSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("a connected project says when its mapping has fallen behind, and changes nothing", async () => {
  // The real case: mapped 2026-08-15, and the rule reading "Waiting On" as
  // blocked landed four days later. Nothing re-applies a recommendation, so the
  // binding quietly got worse as the code got better.
  const requests: { url: string; method: string }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method });
    if (url.includes("/bindings/binding-1/mappings") && method === "GET") return ok([
      { jira_status_id: "1", jira_status_name: "To Do", task_state: "ready" },
      { jira_status_id: "4", jira_status_name: "Waiting On", task_state: "ready" },
    ]);
    if (url.includes("/bindings") && method === "GET") return ok([{
      id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
      scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
      auto_sync_assigned: false,
    }]);
    if (url.includes("/projects/10001/statuses")) return ok([
      { id: "1", name: "To Do", category_key: "new", recommended_task_state: "ready" },
      { id: "4", name: "Waiting On", category_key: "new", recommended_task_state: "blocked" },
    ]);
    if (url.includes("/projects?")) return ok([]);
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));

  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={{ configured: true, accepts_api_token: false, connection: "ready", account_name: "Bea" }}
      unavailable={false}
    />,
  );

  expect(await screen.findByText(/1 of this project's statuses map differently/)).toBeInTheDocument();
  const drift = screen.getByRole("status", { name: "WEB status mapping drift" });
  expect(within(drift).getByText("Waiting On")).toBeInTheDocument();
  expect(within(drift).getByText("Ready")).toBeInTheDocument();
  expect(within(drift).getByText("Blocked")).toBeInTheDocument();
  // The one that still agrees is not reported.
  expect(within(drift).queryByText("To Do")).not.toBeInTheDocument();

  // Reports only. An override may have been deliberate, so nothing is written.
  expect(requests.every((request) => request.method === "GET")).toBe(true);
});

test("discovers a project, maps its workflow, and connects it as a shared Hive pool", async () => {
  const requests: { url: string; method: string; body?: string }[] = [];
  let bound = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    requests.push({ url, method, body: typeof init?.body === "string" ? init.body : undefined });
    if (url.includes("/bindings/binding-1/mappings") && method === "GET") return ok([
      { jira_status_id: "1", jira_status_name: "To Do", task_state: "ready" },
      { jira_status_id: "5", jira_status_name: "Done", task_state: "completed" },
    ]);
    if (url.includes("/bindings") && method === "GET") return ok(bound ? [{
      id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
      scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
      auto_sync_assigned: false,
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
    if (url.includes("/bindings/binding-1/assigned-sync") && method === "PUT") return ok({
      id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
      scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
      auto_sync_assigned: true,
    });
    throw new Error(`Unexpected request: ${method} ${url}`);
  }));

  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={{ configured: true, accepts_api_token: false, connection: "ready", account_name: "Bea" }}
      unavailable={false}
    />,
  );

  fireEvent.change(screen.getByLabelText("Find a Jira project"), { target: { value: "web" } });
  fireEvent.click(await screen.findByRole("option", { name: "WEB Website Services" }));
  expect(await screen.findByText("In Progress")).toBeInTheDocument();
  expect(screen.getByText("Open issues assigned to you synchronize automatically. Unassigned issues remain available to claim from the task board.")).toBeInTheDocument();
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

  expect(screen.queryByRole("button", { name: "Review issues" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("checkbox", { name: /Automatically sync my assigned work/ }));
  expect(await screen.findByText("Website Services will automatically add open Jira work assigned to you.")).toBeInTheDocument();
  const assignedSync = requests.find((request) => request.url.includes("/assigned-sync") && request.method === "PUT");
  expect(JSON.parse(assignedSync?.body ?? "{}")).toEqual({ enabled: true });
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
      readiness={{ configured: true, accepts_api_token: false, connection: "not_connected", account_name: null }}
      unavailable={false}
      onNavigate={assign}
    />,
  );

  expect(screen.getByText("Connect your Atlassian account to choose projects.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Connect with Atlassian" }));
  await waitFor(() => expect(assign).toHaveBeenCalledWith("https://auth.atlassian.test/authorize"));
});

test("a fresh Hive connects Jira with the operator's own token, and is told where to get one", async () => {
  // Reported 2026-08-24 from a first install: the card offered a disabled
  // "Atlassian app setup required" button, because credentials could only come
  // from environment variables at process start. An operator cannot edit a
  // systemd unit from the settings page they are looking at.
  const sent: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/integrations/jira/credentials")) {
      sent.push(JSON.parse(String(init?.body)));
      return ok({ configured: true, accepts_api_token: true, connection: "ready", account_name: "Brad" });
    }
    if (url.endsWith("/api/v1/integrations/jira/bindings")) return ok([]);
    throw new Error(`Unexpected request: ${init?.method ?? "GET"} ${url}`);
  }));

  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={{ configured: false, accepts_api_token: true, connection: "not_connected", account_name: null }}
      unavailable={false}
    />,
  );

  // The way in is named and linked, not described.
  const tokenLinks = screen.getAllByRole("link", { name: /API tokens|api tokens page/i });
  expect(tokenLinks.length).toBeGreaterThan(0);
  expect(tokenLinks[0]).toHaveAttribute("href", "https://id.atlassian.com/manage-profile/security/api-tokens");

  fireEvent.change(screen.getByLabelText("Jira site"), { target: { value: "https://rcg.atlassian.net" } });
  fireEvent.change(screen.getByLabelText("Atlassian email"), { target: { value: "brad@rcg.org" } });
  fireEvent.change(screen.getByLabelText("API token"), { target: { value: "a-real-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Connect Jira" }));

  await waitFor(() => expect(sent).toEqual([
    { base_url: "https://rcg.atlassian.net", email: "brad@rcg.org", api_token: "a-real-token" },
  ]));
  // The token does not stay in the page once it has been handed over.
  await waitFor(() => expect(screen.getByLabelText("API token")).toHaveValue(""));
});

test("keeps a transient binding failure distinct from an empty Jira configuration", async () => {
  let bindingAttempts = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("/bindings")) {
      bindingAttempts += 1;
      if (bindingAttempts === 1) return new Response("upstream unavailable", { status: 502 });
      return ok([{
        id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
        scope: "hive", hive_id: "hive-1", apiary_id: null, access_verified: true, workflow_mapped: true,
        auto_sync_assigned: false,
      }]);
    }
    if (url.includes("/projects?")) return ok([]);
    throw new Error(`Unexpected request: GET ${url}`);
  }));

  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={{ configured: true, accepts_api_token: false, connection: "ready", account_name: "Bea" }}
      unavailable={false}
    />,
  );

  expect(await screen.findByText("Connected projects could not be refreshed")).toBeInTheDocument();
  expect(screen.queryByText("No Jira projects are connected to this Hive yet.")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Website Services")).toBeInTheDocument();
  expect(screen.queryByText("Connected projects could not be refreshed")).not.toBeInTheDocument();
});

test("offers a direct retry when Jira readiness is temporarily unavailable", () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok([])));
  const onRetryReadiness = vi.fn();
  render(
    <JiraSettings
      operatorToken="operator-token"
      readiness={undefined}
      unavailable
      onRetryReadiness={onRetryReadiness}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Retry Jira status" }));
  expect(onRetryReadiness).toHaveBeenCalledOnce();
  expect(screen.getByRole("button", { name: "Connect with Atlassian" })).toBeDisabled();
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}
