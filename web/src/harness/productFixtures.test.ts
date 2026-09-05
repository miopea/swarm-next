import { expect, test } from "vitest";
import { demoBlocked, demoBriefings, demoDecision, demoTasks, demoWorkers } from "./productFixtures";

test("publishable fixtures carry owner and correlation evidence rather than accidental missing-data cases", () => {
  expect(demoTasks.every((task) => Boolean(task.next_move_owner))).toBe(true);
  expect(demoWorkers.some((worker) => worker.id === demoDecision.requesting_worker_id)).toBe(true);
  expect(demoTasks.some((task) => task.id === demoDecision.task_id)).toBe(true);
  expect(demoDecision.delivery_state).toBeNull();
  for (const wait of demoBlocked) {
    expect(demoTasks.find((task) => task.id === wait.task_id)?.state).toBe("blocked");
  }
  expect(new Set(demoBriefings.map((briefing) => briefing.worker_id)).size).toBe(1);
  expect(demoWorkers.some((worker) => worker.id === demoBriefings[0].worker_id)).toBe(true);
});
