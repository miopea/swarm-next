import { expect, test } from "vitest";

import type { Task } from "../api";
import { workerWork } from "./workerWork";

function task(id: string, state: Task["state"], position: number): Task {
  return {
    id, hive_id: "hive", title: `${state} ${id}`, description: "", priority: "normal", workspace: "/repo",
    state, assigned_worker_id: "worker", assigned_session_id: null, position, created_at: position, updated_at: position,
  };
}

test("summarizes unfinished work in operational order", () => {
  const result = workerWork([
    task("draft", "draft", 1),
    task("ready-two", "ready", 2),
    task("done", "completed", 0),
    task("review", "review", 3),
    task("ready-one", "ready", 1),
    task("active", "active", 4),
  ]);

  expect(result.current?.id).toBe("active");
  expect(result.summary).toBe("1 active · 1 review · 2 ready · 1 draft");
});

test("omits a work summary when the worker has no unfinished work", () => {
  expect(workerWork([task("done", "completed", 0)])).toEqual({});
});
