import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { QueenReviewQueueSnapshot } from "../api";
import type { Task } from "../api/tasks";
import { projectReviewQueue } from "./reviewQueueProjection";
import QueuesView from "./QueuesView";

const checkedQueueWaits = (tasks: Task[], value?: QueenReviewQueueSnapshot) => projectReviewQueue(tasks, value).checkedWaits;
const pendingQueueRechecks = (tasks: Task[], value?: QueenReviewQueueSnapshot) => projectReviewQueue(tasks, value).rechecks;

const task = { id: "held", title: "Deliberately deferred work", state: "blocked", next_move_owner: "queen", updated_at: 1, created_at: 1, position: 0, assigned_worker_id: null, prerequisites: [] } as unknown as Task;

test("one queue refresh compares each full task projection once, not once per category", () => {
  const current = { ...task };
  const value = snapshot();
  let taskReads = 0;
  let snapshotReads = 0;
  Object.defineProperty(current, "description", { enumerable: true, get: () => { taskReads++; return "Fictional evidence"; } });
  Object.defineProperty(value.items[0].task, "description", { enumerable: true, get: () => { snapshotReads++; return "Fictional evidence"; } });
  render(<QueuesView tasks={[current]} workers={[]} onOpenTask={vi.fn()} reviewQueue={value} />);
  expect(screen.getByRole("heading", { name: "Scheduled / deliberately parked 1" })).toBeVisible();
  expect(taskReads).toBe(1);
  expect(snapshotReads).toBe(1);
});
function snapshot(status: "covered_for_current_run" | "fresh_external_check_required" | "evidence_changed" | "no_active_review" = "covered_for_current_run", kind: "operator_deferral" | "external_condition" = "operator_deferral"): QueenReviewQueueSnapshot {
  return { checked_at: 10, truncated: false, items: [{ task: { ...task }, previous_assessment: { status, recorded_at: 9,
    assessment: { kind, condition: "Wait for the operator's chosen scope to change", evidence: "Read the original ruling", source: "Existing authenticated decision" } } }] };
}

test("checked waits preserve tasks and reject stale/mixed projections", () => {
  const before = JSON.stringify(task);
  expect(checkedQueueWaits([task], snapshot()).size).toBe(1);
  const reordered = Object.fromEntries(Object.entries(task).reverse()) as unknown as Task;
  expect(checkedQueueWaits([reordered], snapshot()).size).toBe(1);
  for (const changed of [
    { ...task, next_move_owner: "operator" as const },
    { ...task, description: "Changed in the same second" },
    { ...task, assigned_worker_id: "replacement" },
    { ...task, prerequisites: [{ prerequisite_id: "changed" }] } as unknown as Task,
  ]) expect(checkedQueueWaits([changed], snapshot()).size).toBe(0);
  expect(checkedQueueWaits([task], snapshot("evidence_changed")).size).toBe(0);
  expect(checkedQueueWaits([task], snapshot("fresh_external_check_required", "external_condition")).size).toBe(0);
  expect(checkedQueueWaits([task], snapshot("no_active_review", "external_condition")).size).toBe(0);
  expect(checkedQueueWaits([task], snapshot("no_active_review")).size).toBe(1);
  expect(checkedQueueWaits([task]).size).toBe(0);
  expect(JSON.stringify(task)).toBe(before);
});

test("insufficient evidence stays Queen-owned even with a contradictory covered status", () => {
  const value = snapshot();
  const assessment = value.items[0].previous_assessment!;
  assessment.assessment.kind = "insufficient_evidence";
  assessment.assessment.condition = "The original operator statement is unavailable";
  expect(checkedQueueWaits([task], value).size).toBe(0);
  assessment.status = "insufficient_evidence";
  const props = { tasks: [task], workers: [], onOpenTask: vi.fn(), recovery: { items: [], truncated: false } };
  const { rerender } = render(<QueuesView {...props} reviewQueue={value} />);
  expect(screen.getByRole("heading", { name: "Waiting on Queen 1" })).toBeVisible();
  expect(screen.getByText(/Queen still needs evidence: The original operator statement is unavailable/)).toBeVisible();
  fireEvent.click(screen.getByText("What Queen checked"));
  expect(screen.getByText(/This remains Queen's responsibility/)).toBeVisible();
  expect(screen.queryByRole("heading", { name: "Scheduled / deliberately parked 1" })).not.toBeInTheDocument();
  rerender(<QueuesView {...props} reviewQueue={value} coordinatorUnavailable />);
  expect(screen.queryByText(/Queen still needs evidence:/)).not.toBeInTheDocument();
  rerender(<QueuesView {...props} tasks={[{ ...task, description: "Changed evidence" }]} reviewQueue={value} />);
  expect(screen.queryByText(/Queen still needs evidence:/)).not.toBeInTheDocument();
});

test.each(["no_active_review", "fresh_external_check_required"] as const)("external %s is historical context, not queue clearance", status => {
  const value = snapshot(status, "external_condition");
  expect(pendingQueueRechecks([task], value).size).toBe(1);
  expect(checkedQueueWaits([task], value).size).toBe(0);
  const props = { tasks: [task], workers: [], onOpenTask: vi.fn(), recovery: { items: [], truncated: false } };
  const { rerender } = render(<QueuesView {...props} reviewQueue={value} />);
  expect(screen.getByRole("heading", { name: "Waiting on Queen 1" })).toBeVisible();
  expect(screen.getByText("Queen needs to recheck an external condition")).toBeVisible();
  expect(screen.getByText(/Previously waiting for \(not rechecked\):/)).toBeVisible();
  fireEvent.click(screen.getByText(/Previous external check/));
  expect(screen.getByText(/This is historical evidence, not a confirmed current blocker/)).toBeVisible();
  expect(screen.queryByRole("heading", { name: "Blocked on something else 1" })).not.toBeInTheDocument();
  expect(props.onOpenTask).not.toHaveBeenCalled();
  for (const changed of [{ ...task, description: "Changed within the same second" }, { ...task, assigned_worker_id: "other" }]) {
    rerender(<QueuesView {...props} tasks={[changed]} reviewQueue={value} />);
    expect(screen.queryByText(/Previously waiting for/)).not.toBeInTheDocument();
  }
  rerender(<QueuesView {...props} reviewQueue={value} coordinatorUnavailable />);
  expect(screen.queryByText(/Previously waiting for/)).not.toBeInTheDocument();
  rerender(<QueuesView {...props} reviewQueue={snapshot("covered_for_current_run", "external_condition")} />);
  expect(screen.queryByText(/Previously waiting for/)).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Blocked on something else 1" })).toBeVisible();
});

test("recheck context excludes changed evidence, operator deferrals and absent snapshots", () => {
  expect(pendingQueueRechecks([task], snapshot("evidence_changed", "external_condition")).size).toBe(0);
  expect(pendingQueueRechecks([task], snapshot("no_active_review", "operator_deferral")).size).toBe(0);
  expect(pendingQueueRechecks([task]).size).toBe(0);
});

test("parked work keeps a concise reason and source; failed refresh restores recorded ownership", () => {
  const open = vi.fn();
  const props = { tasks: [task], workers: [], onOpenTask: open, recovery: { items: [], truncated: false } };
  const { rerender } = render(<QueuesView {...props} reviewQueue={snapshot()} />);
  expect(screen.queryByRole("heading", { name: "Waiting on Queen 1" })).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Scheduled / deliberately parked 1" })).toBeVisible();
  fireEvent.click(screen.getByText("Show 1 scheduled task"));
  expect(screen.getByText("Operator-deferred · source verified by Queen")).toBeVisible();
  expect(screen.getByText("Waiting for: Wait for the operator's chosen scope to change")).toBeVisible();
  fireEvent.click(screen.getByText(/Queen's check/));
  expect(screen.getByText("Source: Existing authenticated decision")).toBeVisible();
  expect(open).not.toHaveBeenCalled();
  rerender(<QueuesView {...props} reviewQueue={snapshot()} coordinatorUnavailable />);
  expect(screen.getByRole("heading", { name: "Waiting on Queen 1" })).toBeVisible();
  expect(screen.queryByText("Operator-deferred · source verified by Queen")).not.toBeInTheDocument();
  rerender(<QueuesView {...props} reviewQueue={snapshot("covered_for_current_run", "external_condition")} />);
  expect(screen.getByRole("heading", { name: "Blocked on something else 1" })).toBeVisible();
  expect(screen.getByText("External condition · checked by Queen")).toBeVisible();
  rerender(<QueuesView {...props} reviewQueue={{ ...snapshot(), truncated: true }} />);
  expect(screen.getByText(/Queen review details are partial/)).toBeVisible();
});
