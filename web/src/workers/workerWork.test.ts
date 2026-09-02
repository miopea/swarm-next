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

// THE OPERATOR'S TWO SCREENSHOTS, 2026-09-02. The terminal header said
// "AWAITING RELEASE — Two delivery paths…" with a "+31" badge; the roster card
// beside it said "1 review · 6 blocked". They asked which was the error.
//
// Neither and both. The badge counted all 32 open tasks and was right; the
// summary dropped `awaiting_release` entirely, so it described 7 of them; and
// the task the header chose to display was the one the worker was least on.
test("a worker's finished-but-unshipped work is counted, summarised, and never called current", () => {
  const tasks = [
    task("t1", "review", 1),
    task("t2", "blocked", 2),
    ...Array.from({ length: 25 }, (_, index) => task(`r${index}`, "awaiting_release", 10 + index)),
  ];

  const work = workerWork(tasks);

  expect(work.openCount).toBe(27);
  // The summary must account for every open task, or it disagrees with the
  // badge printed next to it by exactly the states it forgot.
  expect(work.summary).toBe("1 review · 25 awaiting_release · 1 blocked");
  // AND THE HEADER MUST NOT LEAD WITH FINISHED WORK. `indexOf` returned -1 for
  // awaiting_release and -1 sorts before 0, so it outranked everything.
  expect(work.current?.state).toBe("review");
});

// The bug generator, not the bug. A state added to the lifecycle and forgotten
// in these arrays used to jump to the FRONT of both orders; it must now fall to
// the back, where being forgotten costs nothing.
test("a state nobody listed sorts last instead of taking over the display", () => {
  const unlisted = task("x", "sideways" as Task["state"], 1);
  const work = workerWork([task("t1", "active", 2), unlisted]);

  expect(work.current?.id).toBe("t1");
  expect(work.openCount).toBe(2);
});

// Abandoned is closed. Counting it as open inflates the badge with work nobody
// will do again — five such tasks sat against this worker when it was found.
test("abandoned work is closed and does not swell the open count", () => {
  const work = workerWork([task("t1", "active", 1), task("t2", "abandoned", 2)]);

  expect(work.openCount).toBe(1);
  expect(work.summary).toBe("1 active");
});
