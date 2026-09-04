import { expect, test, vi } from "vitest";

import {
  assignTask,
  fetchCoordinatorStatus,
  fetchEmailTasksAwaitingReply,
  fetchLocalApiaryTaskExecutions,
  fetchFederationJoinInvitations,
  materializeLocalApiaryTaskExecution,
  recoverTransientRuntime,
  renameApiary,
  renameHive,
  RuntimeRequestError,
} from "./api";

test("background status requests forward cancellation to fetch", async () => {
  const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(async () => new Response("{}", { status: 200 }));
  try {
    const controller = new AbortController();
    await fetchCoordinatorStatus("token", controller.signal);
    await fetchEmailTasksAwaitingReply("token", controller.signal);
    expect(fetch).toHaveBeenCalledTimes(2);
    for (const [, init] of fetch.mock.calls) expect(init?.signal).toBe(controller.signal);
  } finally { fetch.mockRestore(); }
});

test("materializes Keeper work for one private worker through the bounded Apiary API", async () => {
  const execution = {
    apiary_task_id: "apiary-task-1",
    local_task_id: "local-task-1",
    worker_id: "worker-1",
    state: "ready" as const,
    created_at: 10,
  };
  const fetch = vi.fn()
    .mockResolvedValueOnce(new Response(JSON.stringify([execution]), {
      status: 200, headers: { "content-type": "application/json" },
    }))
    .mockResolvedValueOnce(new Response(JSON.stringify(execution), {
      status: 201, headers: { "content-type": "application/json" },
    }));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchLocalApiaryTaskExecutions("operator")).resolves.toEqual([execution]);
  await expect(materializeLocalApiaryTaskExecution(
    "operator", "apiary-task-1", "worker-1",
  )).resolves.toEqual(execution);

  expect(fetch).toHaveBeenNthCalledWith(
    1,
    "/api/v1/apiary/tasks/local-executions",
    expect.objectContaining({ headers: expect.any(Headers) }),
  );
  expect(fetch).toHaveBeenNthCalledWith(
    2,
    "/api/v1/apiary/tasks/apiary-task-1/local-execution",
    expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ worker_id: "worker-1" }),
    }),
  );
  vi.unstubAllGlobals();
});

test("renames public Hive and Apiary labels through bounded private commands", async () => {
  const hive = {
    operator: { id: "operator-1", display_name: "Bea" },
    hive: { id: "hive-1", name: "Clover Hive", operator_id: "operator-1", apiary_id: "apiary-1" },
    apiary_context: {
      mode: "federated" as const,
      apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" as const },
      local_role: "keeper" as const,
    },
  };
  const fetch = vi.fn()
    .mockResolvedValueOnce(new Response(JSON.stringify(hive), { status: 200, headers: { "content-type": "application/json" } }))
    .mockResolvedValueOnce(new Response(JSON.stringify(hive.apiary_context), { status: 200, headers: { "content-type": "application/json" } }));
  vi.stubGlobal("fetch", fetch);

  await expect(renameHive("operator", "Clover Hive")).resolves.toEqual(hive);
  await expect(renameApiary("operator", "Grand Garden")).resolves.toEqual(hive.apiary_context);

  expect(fetch).toHaveBeenNthCalledWith(1, "/api/v1/hive", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ name: "Clover Hive" }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(2, "/api/v1/apiary", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ name: "Grand Garden" }),
  }));
  vi.unstubAllGlobals();
});

test("sends an explicit null worker when returning a task to the queue", async () => {
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({
    id: "task-1", hive_id: "hive-1", title: "Queue me", workspace: "/workspace", state: "ready",
    description: "", priority: "normal", assigned_worker_id: null, assigned_session_id: null,
    position: 0, created_at: 1, updated_at: 1,
  }), { status: 200, headers: { "content-type": "application/json" } }));
  vi.stubGlobal("fetch", fetch);

  await assignTask("operator", "task-1", null);

  expect(fetch).toHaveBeenCalledWith("/api/v1/tasks/task-1/assignment", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ worker_id: null }),
  }));
  vi.unstubAllGlobals();
});

test("recovers a saved browser session after bounded gateway failures", async () => {
  const operation = vi.fn()
    .mockRejectedValueOnce(new RuntimeRequestError(502, "gateway switching"))
    .mockRejectedValueOnce(new TypeError("network unavailable"))
    .mockResolvedValue("restored");

  await expect(recoverTransientRuntime(operation, [0, 0])).resolves.toBe("restored");
  expect(operation).toHaveBeenCalledTimes(3);
});

test("treats a proxy timeout in front of the API as transient", async () => {
  // An operator saw "Runtime request returned 524" while the API was being
  // replaced. Nothing retried it, because only the origin's own 502/503/504
  // counted as transient.
  const operation = vi.fn()
    .mockRejectedValueOnce(new RuntimeRequestError(524, "Runtime request returned 524"))
    .mockResolvedValue("restored");

  await expect(recoverTransientRuntime(operation, [0, 0])).resolves.toBe("restored");
  expect(operation).toHaveBeenCalledTimes(2);
});

test("does not retry invalid credentials", async () => {
  const operation = vi.fn().mockRejectedValue(new RuntimeRequestError(401, "unauthorized"));

  await expect(recoverTransientRuntime(operation, [0, 0])).rejects.toMatchObject({ status: 401 });
  expect(operation).toHaveBeenCalledOnce();
});

test("stops after the bounded recovery budget", async () => {
  const operation = vi.fn().mockRejectedValue(new RuntimeRequestError(503, "runtime unavailable"));

  await expect(recoverTransientRuntime(operation, [0, 0])).rejects.toMatchObject({ status: 503 });
  expect(operation).toHaveBeenCalledTimes(3);
});

test("fails closed when a rolling update briefly returns legacy invitation summaries", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify([{
    invitation_id: "invite-1", apiary_id: "apiary-1", apiary_name: "Garden",
    shared_work_backend: "jira", required_policy_revision: 3,
    promoted_project_catalog_digest: "digest",
    promoted_projects: [{ project_id: "10000", project_key: "WWD", project_name: "Website" }],
    keeper_node_id: "node", keeper_hive_id: "hive", keeper_hive_name: "Rose Hive",
    keeper_operator_id: "operator", keeper_operator_display_name: "Rosa",
    keeper_endpoint: "https://keeper.example.test", state: "keeper_pinned",
    imported_at: 1, expires_at: 2,
  }]), { status: 200, headers: { "content-type": "application/json" } })));

  const [invitation] = await fetchFederationJoinInvitations("operator");

  expect(invitation.readiness_compatibility_fallback).toBe(true);
  expect(invitation.readiness.blockers).toEqual([
    "integration_not_ready", "project_access_not_ready", "policy_not_accepted",
  ]);
  expect(invitation.readiness.projects[0]).toMatchObject({
    binding_id: null, access_verified: false, workflow_mapped: false,
  });
  vi.unstubAllGlobals();
});
