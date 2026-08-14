import { expect, test, vi } from "vitest";

import { assignTask, recoverTransientRuntime, RuntimeRequestError } from "./api";

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
