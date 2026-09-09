import { expect, test } from "vitest";
import type { Task } from "../api/tasks";
import type { HeldBriefing, BlockedEscalation, RecoveryQueueItem } from "../api";
import { groupQueueByWorker, projectTaskQueues } from "./taskQueueProjection";

test("worker groups preserve roster and dispatch order without changing task inputs", () => {
  const workers = [{ id: "b", name: "Bee", position: 0 }, { id: "a", name: "Ant", position: 1 }] as Worker[];
  const source = [task("z", { assigned_worker_id: "a" }), task("later", { assigned_worker_id: "b", position: 8 }),
    task("first", { assigned_worker_id: "b", position: 2 }), task("unassigned"), task("missing", { assigned_worker_id: "gone" })];
  const before = structuredClone(source);
  const groups = groupQueueByWorker(source, workers);
  expect(groups.map(group => group.workerId)).toEqual(["b", "a", "gone", null]);
  expect(groups[0].tasks.map(item => item.id)).toEqual(["first", "later"]);
  expect(groups[2].name).toBe("Worker unavailable (gone)");
  expect(groups[3].name).toBe("Unassigned");
  expect(source).toEqual(before);
});
import type { Worker } from "../api/workers";

const task = (id: string, extra: Partial<Task> = {}): Task => ({
  id, hive_id: "h", title: id, description: "", operator_instruction: "", state: "ready",
  priority: "normal", workspace: "/w", assigned_worker_id: null, assigned_session_id: null,
  dispatch_state: "queued", outcome_delivery_state: null, position: 0, created_at: 1, updated_at: 1,
  next_move_owner: "worker", ...extra,
});
const held = (task_id: string): HeldBriefing => ({ task_id, title: task_id, worker_id: "w", worker_name: "Petal", queued_at: 1, reason: "waiting_its_turn", blocked_by: "Other task" });
const blocked = (task_id: string): BlockedEscalation => ({ task_id, title: task_id, worker_name: "Petal", workspace: "/w", blocked_for_seconds: 100 });

test("recovery counts are exact-session, revision-fenced and do not mutate execution ownership", () => {
  const current = task("recover", { state: "active", dispatch_state: "delivered", assigned_worker_id: "w", assigned_session_id: "s" });
  const check: RecoveryQueueItem = { attention_id: "a", task_id: current.id, worker_id: "w", session_id: "s", task_revision: 1, observed_at: 1, reason: "Resting", state: "verify_worker_response", delivery: null, last_assessment: null };
  const visible = projectTaskQueues([current], [], [], [], [check, check]);
  expect(visible.taskCount).toBe(1);
  expect(visible.activeTasks).toEqual([]);
  expect(visible.waitingTasks).toEqual([current]);
  expect(current.next_move_owner).toBe("worker");
  for (const changed of [{ state: "completed" as const }, { next_move_owner: "operator" as const }, { assigned_worker_id: "other" }, { assigned_session_id: "new" }, { updated_at: 2 }]) {
    expect(projectTaskQueues([{ ...current, ...changed }], [], [], [], [check]).recoveryChecks).toEqual([]);
  }
  expect(projectTaskQueues([current], [], [], [], []).activeTasks).toEqual([current]);
});

test("exact-session input waits remain visible and clear when the worker resumes", () => {
  const current = task("active", { state: "active", dispatch_state: "delivered", assigned_worker_id: "w", assigned_session_id: "s" });
  const worker = { id: "w", running: true, active_session_id: "s", attention_state: "awaiting_operator" } as Worker;
  const waiting = projectTaskQueues([current], [], [], [worker]);
  expect(waiting.waitingTasks).toEqual([current]);
  expect(waiting.activeTasks).toEqual([]);
  expect(waiting.taskCount).toBe(1);
  for (const change of [
    { attention_state: "buzzing" as const }, { attention_state: "resting" as const },
    { id: "other-worker" },
  ]) {
    const projection = projectTaskQueues([current], [], [], [{ ...worker, ...change }]);
    expect(projection.taskCount).toBe(0);
    expect(projection.activeTasks).toEqual([current]);
  }
  expect(current.state).toBe("active");
  expect(current.next_move_owner).toBe("worker");
});

test("known stopped or replaced execution stays visible without inferring an owner change", () => {
  const current = task("active", { state: "active", dispatch_state: "delivered", assigned_worker_id: "w", assigned_session_id: "s" });
  const worker = { id: "w", running: true, active_session_id: "s", attention_state: "resting" } as Worker;
  for (const change of [{ running: false }, { active_session_id: "replacement" }]) {
    const projection = projectTaskQueues([current], [], [], [{ ...worker, ...change }]);
    expect(projection.waitingTasks).toEqual([current]);
    expect(projection.activeTasks).toEqual([]);
    expect(projection.taskCount).toBe(1);
  }
  for (const workers of [[], [worker], [{ id: "w" } as Worker], [{ ...worker, active_session_id: null }]]) {
    expect(projectTaskQueues([current], [], [], workers).activeTasks).toEqual([current]);
  }
  expect(current.next_move_owner).toBe("worker");
  expect(current.state).toBe("active");
});

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

test("reassignment and unassignment discard the former worker's hold without hiding queued work", () => {
  const current = task("ready", { assigned_worker_id: "w" });
  expect(projectTaskQueues([current], [held("ready")], []).heldBriefings).toHaveLength(1);
  for (const assigned_worker_id of ["other-worker", null]) {
    const projection = projectTaskQueues([{ ...current, assigned_worker_id }], [held("ready")], []);
    expect(projection.heldBriefings).toEqual([]);
    expect(projection.waitingTasks).toHaveLength(1);
    expect(projection.taskCount).toBe(1);
  }
  const reassigned = { ...current, assigned_worker_id: "other-worker" };
  expect(projectTaskQueues([reassigned], [{ ...held("ready"), worker_id: "other-worker" }], []).heldBriefings).toHaveLength(1);
});
