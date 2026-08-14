import { expect, test, vi } from "vitest";

import {
  assignTask,
  fetchFederationJoinInvitations,
  recoverTransientRuntime,
  RuntimeRequestError,
} from "./api";

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
