import { afterEach, expect, test, vi } from "vitest";

import {
  addJiraComment,
  beginJiraAuthorization,
  createJiraBinding,
  disconnectJira,
  fetchJiraBindingIssues,
  fetchJiraBindings,
  fetchJiraComments,
  fetchJiraMappings,
  fetchJiraProjects,
  fetchJiraProjectStatuses,
  fetchJiraReadiness,
  fetchJiraTaskLinks,
  reconcileJira,
  replaceJiraMappings,
  retryJiraTaskLink,
  setJiraAssignedSync,
  syncJiraBinding,
} from "../api";

const binding = {
  id: "binding/one",
  project_id: "10000",
  project_key: "WWD",
  project_name: "Website Development",
  scope: "hive" as const,
  hive_id: "hive-1",
  apiary_id: null,
  access_verified: true,
  workflow_mapped: true,
  auto_sync_assigned: true,
};
const mappings = [{ jira_status_id: "1", jira_status_name: "To Do", task_state: "ready" as const }];

afterEach(() => vi.unstubAllGlobals());

function response(payload: unknown): Response {
  return payload === null
    ? new Response(null, { status: 204 })
    : new Response(JSON.stringify(payload), { status: 200, headers: { "content-type": "application/json" } });
}

test("owns bounded Jira readiness, discovery, mapping, issue, link, and comment reads", async () => {
  const payloads = [
    { configured: true, connection: "ready", account_name: "Bea" },
    [{ id: "10000", key: "WWD", name: "Website Development" }],
    [{ id: "1", name: "To Do", category_key: "new", recommended_task_state: "ready" }],
    [binding],
    mappings,
    [{ id: "20000", key: "WWD-1", summary: "Fix", description: "", status_id: "1", status_name: "To Do", assignee_account_id: null, assignee_name: null, updated_at: "now" }],
    [{ id: "comment-1", author_name: "Bea", body: "Context", created_at: "now", updated_at: "now" }],
    { legacy: "invalid-list-shape" },
  ];
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(response(payloads.shift())));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchJiraReadiness("operator")).resolves.toMatchObject({ connection: "ready" });
  await expect(fetchJiraProjects("operator", "  web dev  ")).resolves.toHaveLength(1);
  await expect(fetchJiraProjectStatuses("operator", "WWD/site")).resolves.toHaveLength(1);
  await expect(fetchJiraBindings("operator")).resolves.toEqual([binding]);
  await expect(fetchJiraMappings("operator", "binding/one")).resolves.toEqual(mappings);
  await expect(fetchJiraBindingIssues("operator", "binding/one")).resolves.toHaveLength(1);
  await expect(fetchJiraComments("operator", "task/one")).resolves.toHaveLength(1);
  await expect(fetchJiraTaskLinks("operator")).resolves.toEqual([]);

  expect(fetch).toHaveBeenNthCalledWith(2, "/api/v1/integrations/jira/projects?query=web+dev", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(3, "/api/v1/integrations/jira/projects/WWD%2Fsite/statuses", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(5, "/api/v1/integrations/jira/bindings/binding%2Fone/mappings", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(7, "/api/v1/integrations/jira/task-links/task%2Fone/comments", expect.any(Object));
});

test("serializes Jira authorization, configuration, sync, retry, comment, and reconciliation commands", async () => {
  const task = { id: "task-1", hive_id: "hive-1", title: "Fix", description: "", priority: "normal", workspace: "/projects/web", state: "ready", assigned_worker_id: null, assigned_session_id: null, position: 1, created_at: 1, updated_at: 1 };
  const payloads = [
    { authorization_url: "https://jira.example.test/authorize" },
    null,
    binding,
    mappings,
    binding,
    [task],
    null,
    { state: "queued" },
    null,
  ];
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(response(payloads.shift())));
  vi.stubGlobal("fetch", fetch);

  await expect(beginJiraAuthorization("operator")).resolves.toContain("authorize");
  await disconnectJira("operator");
  await createJiraBinding("operator", { id: "10000", key: "WWD", name: "Website Development" });
  await replaceJiraMappings("operator", "binding/one", mappings);
  await setJiraAssignedSync("operator", "binding/one", true);
  await expect(syncJiraBinding("operator", "binding/one", ["20000"])).resolves.toHaveLength(1);
  await retryJiraTaskLink("operator", "task/one");
  await expect(addJiraComment("operator", "task/one", "Ready for proof")).resolves.toEqual({ state: "queued" });
  await reconcileJira("operator");

  expect(fetch).toHaveBeenNthCalledWith(3, "/api/v1/integrations/jira/bindings", expect.objectContaining({
    method: "POST",
    body: JSON.stringify({ project_id: "10000", project_key: "WWD", project_name: "Website Development" }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(4, "/api/v1/integrations/jira/bindings/binding%2Fone/mappings", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ mappings }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(6, "/api/v1/integrations/jira/bindings/binding%2Fone/sync", expect.objectContaining({
    body: JSON.stringify({ issue_ids: ["20000"] }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(8, "/api/v1/integrations/jira/task-links/task%2Fone/comments", expect.objectContaining({
    body: JSON.stringify({ body: "Ready for proof" }),
  }));
});
