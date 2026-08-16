import { afterEach, expect, test, vi } from "vitest";

import {
  createWorker,
  draftWorkerDescription,
  fetchWorkers,
  fetchWorkspaces,
  removeWorker,
  reorderWorkers,
  startWorker,
  stopWorker,
  updateWorker,
  type Worker,
} from "../api";

const worker: Worker = {
  id: "worker/one",
  hive_id: "hive-1",
  name: "Clover",
  role: "worker",
  provider: "claude_code",
  workspace: "/projects/clover",
  autostart: false,
  position: 1,
  active_session_id: null,
  created_at: 1,
  updated_at: 1,
  running: false,
  attention_state: "sleeping",
};

afterEach(() => vi.unstubAllGlobals());

test("owns worker discovery, configuration, ordering, and lifecycle commands", async () => {
  const responses = [
    [worker],
    [{ name: "clover", path: "/projects/clover", kind: "repository", configured_worker_id: "worker/one" }],
    worker,
    worker,
    null,
    { ...worker, running: true, attention_state: "resting" },
    worker,
    null,
    { description: "Clover owns the test fixture.", source: "repository_metadata" },
  ];
  const fetch = vi.fn().mockImplementation(() => {
    const body = responses.shift();
    return Promise.resolve(body === null
      ? new Response(null, { status: 204 })
      : new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } }));
  });
  vi.stubGlobal("fetch", fetch);

  await expect(fetchWorkers("operator")).resolves.toEqual([worker]);
  await expect(fetchWorkspaces("operator")).resolves.toHaveLength(1);
  await expect(createWorker("operator", {
    name: "Clover",
    workspace: "/outside/clover",
    provider: "claude_code",
    allow_outside_roots: true,
  })).resolves.toEqual(worker);
  await expect(updateWorker("operator", "worker/one", { name: "Clover Bee", autostart: true })).resolves.toEqual(worker);
  await expect(reorderWorkers("operator", ["worker/one", "worker-two"])).resolves.toBeUndefined();
  await expect(startWorker("operator", "worker/one")).resolves.toMatchObject({ running: true });
  await expect(stopWorker("operator", "worker/one")).resolves.toEqual(worker);
  await expect(removeWorker("operator", "worker/one")).resolves.toBeUndefined();
  await expect(draftWorkerDescription("operator", "worker/one")).resolves.toEqual({
    description: "Clover owns the test fixture.",
    source: "repository_metadata",
  });

  expect(fetch).toHaveBeenNthCalledWith(3, "/api/v1/workers", expect.objectContaining({
    method: "POST",
    body: JSON.stringify({
      name: "Clover",
      workspace: "/outside/clover",
      provider: "claude_code",
      allow_outside_roots: true,
    }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(4, "/api/v1/workers/worker%2Fone", expect.objectContaining({
    method: "PATCH",
    body: JSON.stringify({ name: "Clover Bee", autostart: true }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(5, "/api/v1/workers/order", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ worker_ids: ["worker/one", "worker-two"] }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(6, "/api/v1/workers/worker%2Fone/start", expect.objectContaining({
    method: "POST",
    body: JSON.stringify({ rows: 24, columns: 80 }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(7, "/api/v1/workers/worker%2Fone/session", expect.objectContaining({
    method: "DELETE",
  }));
  expect(fetch).toHaveBeenNthCalledWith(8, "/api/v1/workers/worker%2Fone", expect.objectContaining({
    method: "DELETE",
  }));
  expect(fetch).toHaveBeenNthCalledWith(9, "/api/v1/workers/worker%2Fone/description-draft", expect.objectContaining({
    method: "POST",
  }));
});
