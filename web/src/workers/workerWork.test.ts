import { expect, test } from "vitest";

import type { Task } from "../api";
import { workerWork } from "./workerWork";

function task(id: string, state: Task["state"], position: number): Task {
  return {
    id, hive_id: "hive", title: `${state} ${id}`, description: "", operator_instruction: "", priority: "normal", workspace: "/repo",
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
  const work = workerWork([task("done", "completed", 0)]);

  expect(work.summary).toBeUndefined();
  expect(work.current).toBeUndefined();
});

test("counts every open task the worker owns so none stay invisible", () => {
  const work = workerWork([
    task("active", "active", 1),
    task("ready", "ready", 2),
    task("draft", "draft", 3),
    task("done", "completed", 4),
  ]);

  expect(work.current?.id).toBe("active");
  expect(work.openCount).toBe(3);
});

test("reports no open work rather than an absent count", () => {
  expect(workerWork([task("done", "completed", 1)]).openCount).toBe(0);
});

/**
 * The operator, looking at the Swarm Next worker: "why is that one showing as
 * the active task?" It had one blocked task and fifteen ready ones, and none
 * active — and the blocked one was displayed as its current work.
 *
 * Blocked means the worker is waiting on somebody else. It cannot be what the
 * worker is on while there is anything it could actually pick up.
 */
test("a blocked task is not what the worker is on while ready work exists", () => {
  const work = workerWork([
    task("blocked", "blocked", 1),
    task("ready-one", "ready", 2),
    task("ready-two", "ready", 3),
  ]);

  expect(work.current?.id).toBe("ready-one");
  // The summary still reads as a progression, which is a different question.
  expect(work.summary).toBe("1 blocked · 2 ready");
});

/** Same reasoning: work in review is finished and waiting on Queen. */
test("work awaiting review does not outrank work the worker could start", () => {
  const work = workerWork([
    task("review", "review", 1),
    task("ready", "ready", 2),
  ]);

  expect(work.current?.id).toBe("ready");
});

/** Anything genuinely running still wins. */
test("an active task is what the worker is on, whatever else is open", () => {
  const work = workerWork([
    task("review", "review", 1),
    task("blocked", "blocked", 2),
    task("ready", "ready", 3),
    task("active", "active", 4),
  ]);

  expect(work.current?.id).toBe("active");
});

/** With nothing startable, the waiting work is still worth naming. */
test("falls back to blocked work when there is nothing else open", () => {
  const work = workerWork([task("blocked", "blocked", 1)]);

  expect(work.current?.id).toBe("blocked");
});
