import { afterEach, expect, test, vi } from "vitest";

import {
  assignTask,
  createTask,
  fetchRecentTaskActivity,
  fetchTaskActivity,
  fetchTasks,
  reorderTasks,
  transitionTask,
  updateTask,
  type Task,
} from "../api";

const task: Task = {
  id: "task/one",
  hive_id: "hive-1",
  title: "Polish worker states",
  description: "Keep the operator view clear",
  priority: "normal",
  workspace: "/projects/swarm-next",
  state: "ready",
  assigned_worker_id: null,
  assigned_session_id: null,
  position: 1,
  created_at: 1,
  updated_at: 1,
};

const activity = {
  events: [{
    sequence: 1,
    task_id: task.id,
    kind: "created",
    from_state: null,
    to_state: "ready",
    note: "",
    occurred_at: 1,
    actor_kind: "operator",
    actor_id: "operator-1",
  }],
  truncated: false,
};

afterEach(() => vi.unstubAllGlobals());

test("owns core task reads, activity, ordering, editing, transitions, and assignment", async () => {
  const responses = [[task], activity, activity, [task], task, task, task, task];
  const fetch = vi.fn().mockImplementation(() => Promise.resolve(
    new Response(JSON.stringify(responses.shift()), { status: 200, headers: { "content-type": "application/json" } }),
  ));
  vi.stubGlobal("fetch", fetch);

  await expect(fetchTasks("operator")).resolves.toEqual([task]);
  await expect(fetchTaskActivity("operator", "task/one", 12)).resolves.toEqual(activity);
  await expect(fetchRecentTaskActivity("operator", 45)).resolves.toEqual(activity);
  await expect(reorderTasks("operator", ["task/one", "task-two"])).resolves.toEqual([task]);
  await expect(createTask("operator", {
    title: task.title,
    description: task.description,
    priority: task.priority,
    workspace: task.workspace,
  })).resolves.toEqual(task);
  await expect(updateTask("operator", "task/one", { title: "Clear worker states", priority: "high" })).resolves.toEqual(task);
  await expect(transitionTask("operator", "task/one", "active", "Started deliberately")).resolves.toEqual(task);
  await expect(assignTask("operator", "task/one", null)).resolves.toEqual(task);

  expect(fetch).toHaveBeenNthCalledWith(2, "/api/v1/tasks/task%2Fone/activity?limit=12", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(3, "/api/v1/tasks/activity?limit=45", expect.any(Object));
  expect(fetch).toHaveBeenNthCalledWith(4, "/api/v1/tasks/order", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ task_ids: ["task/one", "task-two"] }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(6, "/api/v1/tasks/task%2Fone", expect.objectContaining({
    method: "PATCH",
    body: JSON.stringify({ title: "Clear worker states", priority: "high" }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(7, "/api/v1/tasks/task%2Fone/state", expect.objectContaining({
    method: "PATCH",
    body: JSON.stringify({ state: "active", note: "Started deliberately" }),
  }));
  expect(fetch).toHaveBeenNthCalledWith(8, "/api/v1/tasks/task%2Fone/assignment", expect.objectContaining({
    method: "PUT",
    body: JSON.stringify({ worker_id: null }),
  }));
});
