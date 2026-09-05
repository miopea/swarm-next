import { expect, test } from "vitest";
import type { Task } from "../api/tasks";
import type { HeldBriefing, BlockedEscalation } from "../api";
import { projectTaskQueues } from "./taskQueueProjection";

const task = (id: string, extra: Partial<Task> = {}): Task => ({
  id, hive_id: "h", title: id, description: "", operator_instruction: "", state: "ready",
  priority: "normal", workspace: "/w", assigned_worker_id: null, assigned_session_id: null,
  dispatch_state: "queued", outcome_delivery_state: null, position: 0, created_at: 1, updated_at: 1,
  next_move_owner: "worker", ...extra,
});
const held = (task_id: string): HeldBriefing => ({ task_id, title: task_id, worker_id: "w", worker_name: "Petal", queued_at: 1, reason: "waiting_its_turn", blocked_by: "Other task" });
const blocked = (task_id: string): BlockedEscalation => ({ task_id, title: task_id, worker_name: "Petal", workspace: "/w", blocked_for_seconds: 100 });

test("waiting count excludes ordinary active work but includes unknown owners and uncertain delivery", () => {
  const projection = projectTaskQueues([
    task("active", { state: "active", dispatch_state: "delivered" }),
    task("uncertain", { state: "active", dispatch_state: "uncertain" }),
    task("unknown", { next_move_owner: undefined }),
    task("closed", { state: "completed" }),
  ], [], []);
  expect(projection.taskCount).toBe(2);
  expect(projection.waitingTasks.map((row) => row.id)).toEqual(["uncertain", "unknown"]);
  expect(projection.activeTasks.map((row) => row.id)).toEqual(["active"]);
});

test("counts each task once across canonical and coordinator evidence", () => {
  const projection = projectTaskQueues([task("ready"), task("blocked", { state: "blocked" })],
    [held("ready"), held("extra"), held("extra")], [blocked("blocked"), blocked("extra")]);
  expect(projection.taskCount).toBe(3);
});

test("stale holds cannot resurrect closed work or a cleared block", () => {
  const projection = projectTaskQueues([
    task("closed", { state: "abandoned" }), task("resumed"),
    task("delivered", { state: "active", dispatch_state: "delivered" }),
  ], [held("closed"), held("delivered")], [blocked("closed"), blocked("resumed")]);
  expect(projection.heldBriefings).toEqual([]);
  expect(projection.blockedWaits).toEqual([]);
  expect(projection.taskCount).toBe(1);
});
