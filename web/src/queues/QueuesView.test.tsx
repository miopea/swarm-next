import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import QueuesView from "./QueuesView";
import type { Task } from "../api/tasks";
import type { QueenAutomationStatus, RecoveryQueueItem } from "../api";
import type { Worker } from "../api/workers";

test("clarification names the requester and delivery problem without claiming permission", () => {
  const current = task({ state: "blocked", next_move_owner: "worker", assigned_worker_id: "other",
    clarification_waits: [{ decision_id: "decision", clarification_id: "question", requesting_worker_id: "petal", requester_is_queen: false, delivery_state: "uncertain" }] });
  const props = { tasks: [current], workers: [{ id: "petal", name: "Petal", position: 0 } as Worker], onOpenTask: vi.fn() };
  const { rerender } = render(<QueuesView {...props} />);
  expect(screen.getByRole("heading", { name: "Waiting on a worker 1" })).toBeVisible();
  expect(screen.getByRole("region", { name: "Petal" })).toBeVisible();
  expect(screen.getByText("Waiting for an explanation · original decision still pending")).toBeVisible();
  expect(screen.getByText(/Petal: question delivery unconfirmed/)).toBeVisible();
  rerender(<QueuesView {...props} tasks={[{ ...current, next_move_owner: "operator" }]} />);
  expect(screen.getByRole("heading", { name: "Waiting on you 1" })).toBeVisible();
  expect(screen.getByText("Waiting for your decision")).toBeVisible();
  rerender(<QueuesView {...props} tasks={[{ ...current, state: "active", dispatch_state: "delivered" }]} />);
  expect(screen.getByText("Task active · explanation pending")).toBeVisible();
  expect(screen.getByText(/Petal: question delivery unconfirmed/)).toBeVisible();
});

test.each([
  ["needs_queen_check", "Queen checks unfinished work"],
  ["awaiting_delivery", "Queen's recovery message is waiting for delivery"],
  ["verify_worker_response", "Recovery message delivered · Queen checks the response"],
  ["delivery_needs_recovery", "Queen resolves a failed or uncertain recovery delivery"],
  ["observation_unavailable", "Queen checks unavailable worker evidence"],
] as const)("recovery %s stays with Queen while task execution remains worker-owned", (state, label) => {
  const current = task({ state: "active", next_move_owner: "worker", assigned_worker_id: "w", assigned_session_id: "s", dispatch_state: "delivered" });
  const item: RecoveryQueueItem = { attention_id: "a", task_id: current.id, worker_id: "w", session_id: "s", task_revision: 1, observed_at: 1,
    reason: "The previous turn ended with unfinished work.", state, delivery: { message_id: "message-1", state: "delivered", updated_at: 2 }, last_assessment: { reason: "Previously checked a wait", source: "Fixture" } };
  const onOpenTask = vi.fn();
  const props = { tasks: [current], workers: [{ id: "w", name: "Petal", position: 0 } as Worker], onOpenTask };
  const { rerender } = render(<QueuesView {...props} recovery={{ items: [item], truncated: false }} />);
  expect(screen.getByRole("heading", { name: "Waiting on Queen 1" })).toBeVisible();
  expect(screen.getByText(label)).toBeVisible();
  const deliveryDetails = screen.getByText("delivered · message message-1");
  expect(deliveryDetails).not.toBeVisible();
  const deliverySummary = screen.getByText("Latest recovery delivery: delivered");
  expect(deliverySummary).toBeVisible();
  fireEvent.click(deliverySummary);
  expect(deliveryDetails).toBeVisible();
  expect(screen.queryByText(/Marked active/)).not.toBeInTheDocument();
  fireEvent.click(within(screen.getByRole("region", { name: "Petal" })).getByRole("button"));
  expect(onOpenTask).toHaveBeenCalledWith(current.id);
  expect(current.next_move_owner).toBe("worker");
  rerender(<QueuesView {...props} recovery={{ items: [], truncated: false }} />);
  expect(screen.queryByRole("heading", { name: "Waiting on Queen 1" })).not.toBeInTheDocument();
  expect(screen.getByText(/Marked active/)).toBeVisible();
});

test("missing or partial recovery evidence cannot declare the fleet clear", () => {
  const props = { tasks: [], workers: [], onOpenTask: vi.fn() };
  const { rerender } = render(<QueuesView {...props} />);
  expect(screen.getByText(/Worker recovery details are unavailable/)).toBeVisible();
  expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
  rerender(<QueuesView {...props} recovery={{ items: [], truncated: true }} />);
  expect(screen.getByText(/Recovery details are partial/)).toBeVisible();
  expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
});

function task(overrides: Partial<Task>): Task {
  return {
    id: "t1", hive_id: "h", title: "Some work", description: "", operator_instruction: "",
    workspace: "/w", state: "review", priority: "normal", assigned_worker_id: null,
    assigned_session_id: null, position: 0, created_at: 1, updated_at: 1,
    ...overrides,
  } as Task;
}

describe("QueuesView", () => {
  test("Active execution mismatches are visible and clear when the exact worker session returns", () => {
    const active = task({ state: "active", next_move_owner: "worker", assigned_worker_id: "w", assigned_session_id: "s", dispatch_state: "delivered" });
    const worker = { id: "w", name: "Petal", running: false, active_session_id: null } as Worker;
    const onOpenWorker = vi.fn();
    const props = { tasks: [active], onOpenTask: vi.fn(), onOpenWorker };
    const { rerender } = render(<QueuesView {...props} workers={[worker]} />);
    expect(screen.getByRole("heading", { name: "Waiting on a worker 1" })).toBeVisible();
    expect(screen.getByText("Assigned worker is not running")).toBeVisible();
    rerender(<QueuesView {...props} workers={[{ ...worker, running: true, active_session_id: "replacement" }]} />);
    expect(screen.getByText("Task session differs from the worker's current session · awaiting reconciliation")).toBeVisible();
    rerender(<QueuesView {...props} workers={[{ ...worker, running: true, active_session_id: "s" }]} />);
    expect(screen.queryByRole("heading", { name: "Waiting on a worker 1" })).not.toBeInTheDocument();
    expect(screen.queryByText(/awaiting reconciliation/)).not.toBeInTheDocument();
    expect(screen.getByText("Marked active", { selector: "summary" })).toBeVisible();
    expect(onOpenWorker).not.toHaveBeenCalled();
  });

  test("Ready work exposes observed stopped/waking state and clears it on recovery", () => {
    const ready = task({ state: "ready", next_move_owner: "worker", assigned_worker_id: "w" });
    const worker = { id: "w", name: "Petal", running: false } as Worker;
    const props = { tasks: [ready], onOpenTask: vi.fn(), onOpenWorker: vi.fn() };
    const { rerender } = render(<QueuesView {...props} workers={[worker]} />);
    expect(screen.getByText("Assigned worker is not running")).toBeVisible();
    expect(screen.getByText("Ready · briefing delivery not recorded")).toBeVisible();
    rerender(<QueuesView {...props} workers={[{ ...worker, waking_since: 1 }]} />);
    expect(screen.getByText("Worker wake queued or in progress")).toBeVisible();
    expect(screen.queryByText("Assigned worker is not running")).not.toBeInTheDocument();
    for (const workers of [[{ ...worker, running: true }], [], [{ id: "w", name: "Petal" } as Worker]]) {
      rerender(<QueuesView {...props} workers={workers} />);
      expect(screen.queryByText("Assigned worker is not running")).not.toBeInTheDocument();
      expect(screen.queryByText("Worker wake queued or in progress")).not.toBeInTheDocument();
    }
    rerender(<QueuesView {...props} workers={[worker]} tasks={[{ ...ready, next_move_owner: "operator" }]} />);
    expect(screen.getByText("Waiting for your decision")).toBeVisible();
    expect(screen.queryByText("Assigned worker is not running")).not.toBeInTheDocument();
    expect(props.onOpenWorker).not.toHaveBeenCalled();
    expect(props.onOpenTask).not.toHaveBeenCalled();
  });

  test.each(["queued", "dispatching", "delivered", "uncertain"] as const)("operator-owned Ready work explains the decision before %s briefing history", (dispatch_state) => {
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[task({ state: "ready", next_move_owner: "operator", dispatch_state })]} />);
    expect(screen.getByRole("heading", { name: "Waiting on you 1" })).toBeVisible();
    expect(screen.getByText("Waiting for your decision")).toBeVisible();
    expect(screen.queryByText(/Briefing (delivered|awaiting|delivery unconfirmed)/)).not.toBeInTheDocument();
  });
  test("a returned review with an operator decision shows the human next move", () => {
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[task({
      state: "review", next_move_owner: "operator", review_request_id: "request",
      review_request: "Verify using an approved test account", assigned_worker_id: "w",
    })]} />);
    expect(screen.getByRole("heading", { name: "Waiting on you 1" })).toBeVisible();
    expect(screen.getByText("Waiting for your decision")).toBeVisible();
    expect(screen.queryByRole("heading", { name: /Waiting on a worker/ })).not.toBeInTheDocument();
  });
  test("each owner section groups workers and shows recorded order and review waits", () => {
    const workers = [{ id: "b", name: "Bee", position: 0 }, { id: "a", name: "Ant", position: 1 }] as Worker[];
    render(<QueuesView workers={workers} onOpenTask={vi.fn()} tasks={[
      task({ id: "later", title: "Later task", assigned_worker_id: "b", position: 8, next_move_owner: "worker", review_request_id: "r", review_request: "Verify the mobile download" }),
      task({ id: "ant", assigned_worker_id: "a", next_move_owner: "worker" }),
      task({ id: "first", title: "Earlier task", assigned_worker_id: "b", position: 1, next_move_owner: "worker" }),
      task({ id: "draft", state: "draft", next_move_owner: "queen" }),
    ]} />);
    const bee = screen.getByRole("region", { name: "Bee" });
    expect(within(bee).getAllByRole("button").map(button => button.textContent)).toEqual([
      expect.stringContaining("Earlier task"), expect.stringContaining("Later task"),
    ]);
    expect(within(bee).getByText("Waiting for the worker to answer Queen's review request")).toBeVisible();
    expect(within(bee).getByText("Queen asks: Verify the mobile download")).toBeVisible();
    expect(screen.getByRole("region", { name: "Unassigned" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Waiting on Queen 1" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Waiting on a worker 3" })).toBeVisible();
  });
  test("a current terminal input wait stays visible without assigning it to the operator", () => {
    const current = task({ state: "active", next_move_owner: "worker", dispatch_state: "delivered", assigned_worker_id: "w", assigned_session_id: "s" });
    const worker = { id: "w", name: "Petal", running: true, active_session_id: "s", attention_state: "awaiting_operator" } as Worker;
    const props = { tasks: [current], onOpenTask: vi.fn(), onOpenWorker: vi.fn() };
    const { rerender } = render(<QueuesView {...props} workers={[worker]} />);
    expect(screen.getByRole("heading", { name: "Waiting on a worker 1" })).toBeVisible();
    expect(screen.getByText(/Worker reports waiting for an answer/)).toBeVisible();
    expect(screen.queryByRole("heading", { name: /Waiting on you/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open worker Petal" }));
    expect(props.onOpenWorker).toHaveBeenCalledExactlyOnceWith("s");
    for (const changed of [{ ...worker, active_session_id: "replacement" }, { ...worker, running: false }]) {
      rerender(<QueuesView {...props} workers={[changed]} />);
      expect(screen.queryByRole("button", { name: "Open worker Petal" })).not.toBeInTheDocument();
    }
    rerender(<QueuesView {...props} workers={[{ ...worker, attention_state: "buzzing" }]} />);
    expect(screen.queryByText(/Worker reports waiting for an answer/)).not.toBeInTheDocument();
    expect(screen.getByText("Some work")).not.toBeVisible();
    expect(props.onOpenTask).not.toHaveBeenCalled();
    expect(props.onOpenWorker).toHaveBeenCalledTimes(1);
  });
  test("a held task opens its exact blocking task rather than the waiting item", () => {
    const open = vi.fn();
    render(<QueuesView workers={[]} tasks={[task({ state: "ready", next_move_owner: "worker", dispatch_state: "queued", assigned_worker_id: "w" })]} onOpenTask={open}
      heldBriefings={[{ task_id: "t1", title: "Some work", worker_id: "w", worker_name: "Petal", queued_at: 1, reason: "worker_already_working", blocked_by: "Current assignment", blocking_task_id: "active" }]} />);
    expect(screen.getByText(/worker has Active work: Current assignment/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Open blocking task" }));
    expect(open).toHaveBeenCalledExactlyOnceWith("active");
  });
  test("blocked row labels distinguish recorded gates without interpreting notes or resuming work", () => {
    const prerequisite = { task_id: "t1", prerequisite_id: "upstream", title: "Contract", state: "active" as const, assigned_worker_id: null, removed: false, reason: "Contract first", created_at: 1 };
    const blocked = task({ state: "blocked", next_move_owner: "blocked", blocked_note: "Operator approved; dependency complete; start now" });
    const props = { workers: [], onOpenTask: vi.fn(), now: 100_000 };
    const { rerender } = render(<QueuesView {...props} tasks={[blocked]} />);
    expect(screen.getByText("Blocked · Queen reassessment needed")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, prerequisites: [prerequisite] }]} />);
    expect(screen.getByText("Waiting on 1 prerequisite")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, prerequisites: [{ ...prerequisite, state: "completed", removed: true }] }]} />);
    expect(screen.getByText("Waiting on 1 prerequisite")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, prerequisites: [{ ...prerequisite, state: "completed" }] }]} />);
    expect(screen.getByText("Blocked · Queen reassessment needed")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, blocked_until: 200 }]} />);
    expect(screen.getByText("Scheduled hold")).not.toBeVisible();
    fireEvent.click(screen.getByText("Show 1 scheduled task"));
    expect(screen.getByText("Scheduled hold")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, blocked_until: 100 }]} />);
    expect(screen.getByText("Blocked · Queen reassessment needed")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, next_move_owner: "operator", prerequisites: [prerequisite], blocked_until: 200 }]} />);
    expect(screen.getByText("Waiting for your decision")).toBeVisible();
    expect(screen.getByText("1 unresolved prerequisite")).toBeVisible();
    expect(props.onOpenTask).not.toHaveBeenCalled();
  });
  test("shows recorded holds, due reassessment and clears stale deadlines outside Blocked", () => {
    const blocked = task({ state: "blocked", next_move_owner: "blocked", blocked_until: 200 });
    const props = { workers: [], onOpenTask: vi.fn() };
    const { rerender } = render(<QueuesView {...props} tasks={[blocked]} now={100_000} />);
    fireEvent.click(screen.getByText("Show 1 scheduled task"));
    expect(screen.getByText(`Scheduled hold until ${new Date(200_000).toLocaleString()}`)).toBeVisible();
    rerender(<QueuesView {...props} tasks={[blocked]} now={200_000} />);
    expect(screen.getByText("Recorded hold ended · Queen reassesses remaining blockers")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, state: "ready", next_move_owner: "worker" }]} now={200_000} />);
    expect(screen.queryByText(/Recorded hold|Scheduled hold/)).not.toBeInTheDocument();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, blocked_until: null, blocked_note: "Maybe tomorrow" }]} now={200_000} />);
    expect(screen.queryByText(/Recorded hold|Scheduled hold/)).not.toBeInTheDocument();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, blocked_until: Number.NaN }]} now={200_000} />);
    expect(screen.getByText("Recorded hold deadline unavailable")).toBeVisible();
  });

  test("only recorded future holds are parked, while operator decisions and missing evidence remain visible", () => {
    const onOpenTask = vi.fn();
    const scheduled = task({ id: "later", title: "Wait for the approved window", state: "blocked", next_move_owner: "blocked", blocked_until: 200 });
    const decision = task({ id: "you", title: "Operator choice", state: "blocked", next_move_owner: "operator", blocked_until: 200 });
    const unknown = task({ id: "unknown", title: "No recorded owner", state: "blocked", next_move_owner: undefined, blocked_until: 200 });
    const noteOnly = task({ id: "note", title: "Prose is not a clock", state: "blocked", next_move_owner: "blocked", blocked_note: "Park until next year" });
    const tasks = [scheduled, decision, unknown, noteOnly];
    const { rerender } = render(<QueuesView tasks={tasks} workers={[]} onOpenTask={onOpenTask} now={100_000} />);
    expect(screen.getByRole("link", { name: "Scheduled 1" })).toBeVisible();
    expect(screen.getByText(scheduled.title)).not.toBeVisible();
    expect(screen.getByText(decision.title)).toBeVisible();
    expect(screen.getByText(unknown.title)).toBeVisible();
    expect(screen.getByText(noteOnly.title)).toBeVisible();
    rerender(<QueuesView tasks={tasks} workers={[]} onOpenTask={onOpenTask} now={200_000} />);
    expect(screen.queryByRole("link", { name: "Scheduled 1" })).not.toBeInTheDocument();
    expect(screen.getByText(scheduled.title)).toBeVisible();
    expect(onOpenTask).not.toHaveBeenCalled();
    expect(scheduled.state).toBe("blocked");
  });
  test("shows Queen's recorded pacing without inventing a task or retaining it after progress", () => {
    const status: QueenAutomationStatus = { enabled: true, state: "queued", run_id: "run", trigger: "actionable_work", actionable_count: 1, attempts: 0, requested_at: 1, delivered_at: null, finished_at: null, outcome: null, waiting_reason: "Pacing Queen's next review after a recent delivery" };
    const props = { tasks: [], workers: [], onOpenTask: vi.fn() };
    const { rerender } = render(<QueuesView {...props} queenAutomation={status} />);
    expect(screen.getByText(status.waiting_reason!)).toBeVisible();
    expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
    expect(screen.queryByRole("article")).not.toBeInTheDocument();
    rerender(<QueuesView {...props} tasks={[task({ next_move_owner: "queen" })]} queenAutomation={status} />);
    expect(screen.getAllByText(status.waiting_reason!)).toHaveLength(1);
    rerender(<QueuesView {...props} queenAutomation={{ ...status, state: "running", waiting_reason: null }} />);
    expect(screen.queryByText(status.waiting_reason!)).not.toBeInTheDocument();
  });
  test("shows observed continuation exhaustion without treating a running review as complete", () => {
    const reason = "Automatic continuation paused: Queen was just observed idle after using this review's delivery budget.";
    const status: QueenAutomationStatus = { enabled: true, state: "running", run_id: "same-run", trigger: "actionable_work", actionable_count: 1, attempts: 3, requested_at: 1, delivered_at: 2, finished_at: null, outcome: null, waiting_reason: reason };
    const props = { tasks: [], workers: [], onOpenTask: vi.fn() };
    const { rerender } = render(<QueuesView {...props} queenAutomation={status} />);
    expect(screen.getByText(reason)).toBeVisible();
    expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
    rerender(<QueuesView {...props} queenAutomation={{ ...status, waiting_reason: null }} />);
    expect(screen.queryByText(reason)).not.toBeInTheDocument();
  });
  test("explicit prerequisites link to their task and completion waits for Queen", () => {
    const prerequisite = { task_id: "t1", prerequisite_id: "upstream", title: "Shared API contract", state: "active" as const, assigned_worker_id: null, removed: false, reason: "The response shape must be settled first", created_at: 1 };
    const blocked = task({ state: "blocked", next_move_owner: "blocked", prerequisites: [prerequisite] });
    const onOpenTask = vi.fn();
    const { rerender } = render(<QueuesView tasks={[blocked]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.getByText("1 unresolved prerequisite")).toBeVisible();
    expect(screen.getByText(/No worker assigned/)).toBeVisible();
    const explanation = screen.getByText(prerequisite.reason);
    expect(explanation).not.toBeVisible();
    expect(explanation.closest("details")).not.toHaveAttribute("open");
    fireEvent.click(screen.getByText("Why this dependency"));
    expect(explanation).toBeVisible();
    expect(screen.queryByText("Blocked · reason not recorded")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: prerequisite.title }));
    expect(onOpenTask).toHaveBeenCalledWith("upstream");
    rerender(<QueuesView tasks={[{ ...blocked, next_move_owner: "queen", prerequisites: [{ ...prerequisite, state: "completed" }] }]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.getByText("Prerequisites completed · Queen checks remaining blockers before resuming")).toBeVisible();
    expect(screen.getByText("1 completed prerequisite").closest("details")).not.toHaveAttribute("open");
    expect(screen.queryByText("1 unresolved prerequisite")).not.toBeInTheDocument();
    rerender(<QueuesView tasks={[{ ...blocked, next_move_owner: "operator", prerequisites: [{ ...prerequisite, state: "completed" }] }]} workers={[]} onOpenTask={onOpenTask}
      blockedWaits={[{ task_id: "t1", title: "Some work", worker_name: "Worker", workspace: "/w", blocked_for_seconds: 7200 }]} />);
    expect(screen.getByRole("heading", { name: "Waiting on you 1" })).toBeVisible();
    expect(screen.getByText("Prerequisites completed · your decision is still needed")).toBeVisible();
    expect(screen.getByText("Blocked for 2h")).toBeVisible();
    expect(screen.queryByText(/Queen (checks remaining blockers|coordinates the next move)/)).not.toBeInTheDocument();
  });

  test("review dependencies explain the wait without hiding the operator's next move", () => {
    const prerequisite = { task_id: "t1", prerequisite_id: "upstream", title: "Shared test session", state: "active" as const, assigned_worker_id: null, removed: false, reason: "Real session verification", created_at: 1 };
    const waiting = task({ state: "review", next_move_owner: "blocked", prerequisites: [prerequisite] });
    const onOpenTask = vi.fn();
    const { rerender } = render(<QueuesView tasks={[waiting]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.getByText("Review waiting on 1 prerequisite")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: prerequisite.title }));
    expect(onOpenTask).toHaveBeenCalledWith("upstream");
    rerender(<QueuesView tasks={[{ ...waiting, next_move_owner: "operator" }]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.getByText("Waiting for your decision")).toBeVisible();
  });

  test("removed and reopened prerequisites stay visible without claiming the worker stopped", () => {
    const prerequisite = { task_id: "t1", prerequisite_id: "upstream", title: "Shared API contract", state: "completed" as const, assigned_worker_id: null, removed: true, reason: "Contract first", created_at: 1 };
    render(<QueuesView tasks={[task({ state: "active", next_move_owner: "worker", dispatch_state: "delivered", prerequisites: [prerequisite] })]} workers={[]} onOpenTask={vi.fn()} />);
    expect(screen.getByText(/Queen must reconcile; running work has not been stopped/)).toBeVisible();
    expect(screen.getByText(/Removed · Queen must reconcile/)).toBeVisible();
    expect(screen.queryByRole("button", { name: prerequisite.title })).not.toBeInTheDocument();
    expect(screen.queryByText("Marked active", { selector: "summary" })).not.toBeInTheDocument();
  });
  test("a held task appears once with its recorded reason and clears that reason after delivery", () => {
    const ready = task({ state: "ready", next_move_owner: "worker", dispatch_state: "queued", assigned_worker_id: "worker" });
    const props = { workers: [], onOpenTask: vi.fn(), now: 120_000, heldBriefings: [{
      task_id: ready.id, title: ready.title, worker_id: "worker", worker_name: "Orchard",
      reason: "waiting_its_turn", blocked_by: "Earlier task", queued_at: 60,
    }] };
    const { rerender } = render(<QueuesView {...props} tasks={[ready]} />);
    expect(screen.getAllByText(ready.title)).toHaveLength(1);
    expect(screen.getByText("Briefing held: behind Earlier task · queued at least 1 minute")).toBeVisible();
    expect(screen.queryByText("One briefing is queued")).not.toBeInTheDocument();
    rerender(<QueuesView {...props} tasks={[{ ...ready, dispatch_state: "delivered" }]} />);
    expect(screen.queryByText(/Briefing held:/)).not.toBeInTheDocument();
    expect(screen.getAllByText(ready.title)).toHaveLength(1);
    rerender(<QueuesView {...props} tasks={[]} />);
    expect(screen.getByText("One briefing is queued")).toBeVisible();
    expect(screen.getAllByText(ready.title)).toHaveLength(1);
  });
  test("shows the recorded blocker and removes it when work resumes", () => {
    const blocked = task({ state: "blocked", next_move_owner: "blocked", blocked_note: "Waiting for the API contract" });
    const props = { workers: [], onOpenTask: vi.fn() };
    const { rerender } = render(<QueuesView {...props} tasks={[blocked]} />);
    expect(screen.getByText("Recorded when blocked: Waiting for the API contract")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, state: "ready", next_move_owner: "worker" }]} />);
    expect(screen.queryByText(/Recorded when blocked/)).not.toBeInTheDocument();
    rerender(<QueuesView {...props} tasks={[{ ...blocked, blocked_note: null }]} />);
    expect(screen.getByText("Blocked · reason not recorded")).toBeVisible();
    expect(screen.queryByText(/Waiting for the API contract/)).not.toBeInTheDocument();
  });

  test("long blocker notes have a concise preview with the full statement available", () => {
    const note = "Waiting for the shared contract. ".repeat(20);
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[task({ state: "blocked", next_move_owner: "blocked", blocked_note: note })]} />);
    const details = screen.getByText(note.trim()).closest("details");
    expect(details).not.toHaveAttribute("open");
    expect(details?.querySelector("summary")).toHaveTextContent(`Recorded when blocked: ${note.slice(0, 96)}…`);
  });

  test("Unicode evidence previews bound character allocation without truncating the full statement", () => {
    const note = "🐝".repeat(4000);
    const from = vi.spyOn(Array, "from");
    try {
      const { container } = render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[task({ state: "blocked", next_move_owner: "blocked", blocked_note: note })]} />);
      const strings = from.mock.calls.map(([input]) => input).filter((input): input is string => typeof input === "string" && input.startsWith("🐝"));
      expect(strings.length).toBeGreaterThan(0);
      expect(Math.max(...strings.map(input => input.length))).toBeLessThanOrEqual(194);
      expect(container.querySelector(".queue-evidence summary")?.textContent).toBe(`Recorded when blocked: ${"🐝".repeat(96)}…`);
      expect(container.querySelector(".decision-prose")?.textContent).toBe(note);
    } finally { from.mockRestore(); }
  });

  test("medium review questions collapse without losing the exact full evidence", () => {
    const question = "Verify the worker's recorded test result before proceeding. ".repeat(3);
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[task({
      state: "review", next_move_owner: "worker", review_request_id: "review-1", review_request: question,
    })]} />);
    const details = screen.getByText(question.trim()).closest("details");
    expect(details).not.toHaveAttribute("open");
    expect(details?.querySelector("summary")).toHaveTextContent(`Queen asks: ${question.slice(0, 96)}…`);
    expect(details?.querySelector(".decision-prose")?.textContent).toBe(question);
  });

  test("ordinary active work is inspectable but collapsed outside waiting groups", () => {
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[
      task({ id: "active", title: "Doing the work", state: "active", next_move_owner: "worker", dispatch_state: "delivered" }),
      task({ id: "review", title: "Answer Queen", next_move_owner: "worker" }),
    ]} />);
    const active = screen.getByText("Doing the work").closest("details");
    expect(active).not.toBeNull();
    expect(active).not.toHaveAttribute("open");
    expect(active?.querySelector("summary")).toHaveTextContent("Marked active 1");
    expect(screen.getByRole("heading", { name: "Waiting on a worker 1" })).toBeVisible();
    expect(screen.getByText("Answer Queen")).toBeVisible();
  });

  test("active delivery problems remain visible rather than folded into ordinary work", () => {
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[
      task({ state: "active", next_move_owner: "worker", dispatch_state: "uncertain" }),
    ]} />);
    expect(screen.getByText(/Briefing delivery unconfirmed/)).toBeVisible();
    expect(screen.getByText("Some work").closest("details")).toBeNull();
  });

  test("an active-only queue keeps its collapsed inspection instead of claiming no work", () => {
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[
      task({ state: "active", next_move_owner: "worker", dispatch_state: "delivered" }),
    ]} />);
    expect(screen.getByText("Some work").closest("details")).not.toBeNull();
    expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
  });
  test("long review requests keep their complete text in a collapsed disclosure", () => {
    const question = "Verify the failure case. ".repeat(30);
    render(<QueuesView workers={[]} onOpenTask={vi.fn()} tasks={[task({ next_move_owner: "worker", review_request_id: "request-long", review_request: question })]} />);
    const disclosure = screen.getByText(question.trim()).closest("details");
    expect(disclosure).not.toHaveAttribute("open");
    expect(disclosure?.querySelector("summary")).toHaveTextContent("Queen asks:");
    expect(disclosure?.querySelector("p")?.textContent).toBe(question);
  });
  test("shows the exact current review question and clears it when answered", () => {
    const request = task({ next_move_owner: "worker", review_request_id: "request-1", review_request: "Which SHA?" });
    const props = { workers: [], onOpenTask: vi.fn() };
    const { rerender } = render(<QueuesView {...props} tasks={[request]} />);
    expect(screen.getByText("Queen asks: Which SHA?")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...request, next_move_owner: "queen", review_request_id: null, review_request: null }]} />);
    expect(screen.queryByText("Queen asks: Which SHA?")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Waiting on Queen/ })).toBeVisible();
  });
  test("shows delivery progression without claiming a queued worker is actively working", () => {
    const props = { workers: [], onOpenTask: vi.fn() };
    const ready = task({ state: "ready", next_move_owner: "worker", dispatch_state: "queued" });
    const { rerender } = render(<QueuesView {...props} tasks={[ready]} />);
    expect(screen.getByText("Briefing awaiting confirmed delivery")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...ready, dispatch_state: "delivered" }]} />);
    expect(screen.getByText("Briefing delivered · work has not been marked active")).toBeVisible();
    expect(screen.queryByText("Briefing awaiting confirmed delivery")).not.toBeInTheDocument();
    rerender(<QueuesView {...props} tasks={[{ ...ready, dispatch_state: "uncertain" }]} />);
    expect(screen.getByText("Briefing delivery unconfirmed · Queen must reconcile before retrying")).toBeVisible();
    rerender(<QueuesView {...props} tasks={[{ ...ready, state: "completed", next_move_owner: "nobody" }]} />);
    expect(screen.queryByText(/Briefing delivery unconfirmed/)).not.toBeInTheDocument();
  });

  test("distinguishes pending review transport and does not label update age as wait age", () => {
    render(<QueuesView tasks={[task({ next_move_owner: "queen", outcome_delivery_state: "queued" })]} workers={[]} onOpenTask={vi.fn()} now={3_600_000} />);
    expect(screen.getByText("Review handoff awaiting confirmed delivery")).toBeVisible();
    expect(screen.getByText(/Longest since task update/)).toBeVisible();
    expect(screen.queryByText(/^Oldest /)).not.toBeInTheDocument();
  });
  test("an exact Queen run keeps its owner and reason visible without expanding details", () => {
    render(<QueuesView tasks={[]} workers={[]} onOpenTask={vi.fn()} heldDeliveries={[{
      kind: "delivery_held_unsent_text", subject: "queen-run:current-run", worker_name: null,
      reason: "The prompt contains an unsent operator draft", first_observed_at: 1, observations: 2,
    }]} />);
    expect(screen.getByRole("heading", { name: "Queen" })).toBeInTheDocument();
    expect(screen.getByText("The prompt contains an unsent operator draft")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Unknown worker" })).not.toBeInTheDocument();
  });
  test("assigns uncertain message reconciliation to Queen without claiming a stopped prompt", () => {
    const props = { tasks: [], workers: [], onOpenTask: vi.fn() };
    const { rerender } = render(<QueuesView {...props} heldDeliveries={[{
      kind: "task_message_reconciliation", subject: "message-1", worker_name: "Queen",
      reason: "Inspect the saved message before retrying", first_observed_at: 1, last_observed_at: 1, observations: 1,
    }]} />);
    expect(screen.getByRole("heading", { name: "Queen" })).toBeInTheDocument();
    expect(screen.getByText("Queen: reconcile message delivery")).toBeInTheDocument();
    expect(screen.queryByText("Last observed hold: prompt not ready")).not.toBeInTheDocument();
    rerender(<QueuesView {...props} heldDeliveries={[]} recovery={{ items: [], truncated: false }} />);
    expect(screen.queryByText("Queen: reconcile message delivery")).not.toBeInTheDocument();
  });
  test("retains delivery evidence without claiming the Queen has stopped, then clears it", () => {
    const props = { tasks: [], workers: [], onOpenTask: vi.fn(), recovery: { items: [], truncated: false } };
    const { rerender } = render(<QueuesView {...props} heldDeliveries={[{
      kind: "delivery_held_unsent_text", subject: "queen-review", worker_name: null,
      reason: "The last observed prompt contained text", first_observed_at: 1, observations: 1503,
    }]} />);
    expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Queen" })).toBeInTheDocument();
    expect(screen.getByText("Last observed hold: unsent text")).toBeInTheDocument();
    expect(screen.getByText(/Last observation time unavailable/)).toBeInTheDocument();
    expect(screen.queryByText(/Nothing gets routed/)).not.toBeInTheDocument();
    rerender(<QueuesView {...props} heldDeliveries={[{
      kind: "delivery_held_unsent_text", subject: "queen-review", worker_name: null,
      reason: "The last observed prompt contained text", first_observed_at: 1, last_observed_at: 10, observations: 1503,
    }]} />);
    expect(screen.getByText(`Last observed ${new Date(10_000).toLocaleString()}. No resolution has been confirmed.`)).toBeInTheDocument();
    rerender(<QueuesView {...props} heldDeliveries={[]} />);
    expect(screen.queryByText("Last observed hold: unsent text")).not.toBeInTheDocument();
    expect(screen.getByText("Nothing is waiting on anyone.")).toBeInTheDocument();
  });
  /**
   * The whole point: a pile is attributable. Grouping by mechanism would put
   * one stall in several places and answer "why is nothing moving" with a
   * shrug.
   */
  test("groups open work by who owes the next move", () => {
    render(<QueuesView onOpenTask={vi.fn()} workers={[]} tasks={[
      task({ id: "a", title: "Judge me", next_move_owner: "queen" }),
      task({ id: "b", title: "Judge me too", next_move_owner: "queen" }),
      task({ id: "c", title: "Mine", state: "active", next_move_owner: "worker" }),
      task({ id: "d", title: "Stuck", state: "blocked", next_move_owner: "blocked" }),
      task({ id: "e", title: "Needs a ruling", state: "blocked", next_move_owner: "operator" }),
    ]} />);

    expect(screen.getByRole("heading", { name: /Waiting on Queen 2/ })).toBeInTheDocument();
    expect(screen.getByText("Mine").closest("details")?.querySelector("summary")).toHaveTextContent("Marked active 1");
    expect(screen.getByRole("heading", { name: /Blocked on something else 1/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Waiting on you 1/ })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /Next owner not recorded/ })).not.toBeInTheDocument();
  });

  /**
   * Closed work is not a queue. Including it would make every group grow
   * forever and the counts would stop meaning anything.
   */
  test("closed work is not a queue", () => {
    render(<QueuesView onOpenTask={vi.fn()} workers={[]} recovery={{ items: [], truncated: false }} tasks={[
      task({ id: "a", state: "completed", next_move_owner: "nobody" }),
      task({ id: "b", state: "abandoned", next_move_owner: "nobody" }),
    ]} />);
    expect(screen.getByText("Nothing is waiting on anyone.")).toBeInTheDocument();
  });

  /**
   * An older server omits the field. Nothing is invented for it: a task with no
   * stated owner is left out rather than attributed to somebody who would then
   * carry a queue that is not theirs.
   */
  test("work whose owner the server did not state is not attributed to anyone", () => {
    render(<QueuesView onOpenTask={vi.fn()} workers={[]} tasks={[
      task({ id: "a", title: "Unknown owner" }),
    ]} />);
    expect(screen.queryByText("Nothing is waiting on anyone.")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Next owner not recorded/ })).toBeInTheDocument();
    expect(screen.getByText("Unknown owner")).toBeInTheDocument();
  });

  test("blocked age is queue evidence without duplicating or resurrecting tasks", () => {
    const wait = { task_id: "a", title: "Blocked task", worker_name: "Orchard", workspace: "/w", blocked_for_seconds: 50_000 };
    render(<QueuesView onOpenTask={vi.fn()} workers={[]} tasks={[
      task({ id: "a", title: "Blocked task", state: "blocked", next_move_owner: "blocked" }),
      task({ id: "b", title: "Resolved task", state: "completed", next_move_owner: "nobody" }),
    ]} blockedWaits={[wait, { ...wait, task_id: "b", title: "Resolved task" }]} />);
    expect(screen.getAllByText("Blocked task")).toHaveLength(1);
    expect(screen.getByText(/Blocked for 13h/)).toBeInTheDocument();
    expect(screen.queryByText("Resolved task")).not.toBeInTheDocument();
  });

  test("owner navigation matches visible groups and updates without moving or changing tasks", () => {
    const onOpenTask = vi.fn();
    const waiting = [task({ id: "q", next_move_owner: "queen" }), task({ id: "b", state: "blocked", next_move_owner: "blocked" })];
    const { rerender } = render(<QueuesView tasks={waiting} workers={[]} onOpenTask={onOpenTask} />);
    const index = screen.getByRole("navigation", { name: "Jump to queue owner" });
    const queenLink = within(index).getByRole("link", { name: "Queen 1" });
    const target = document.getElementById(queenLink.getAttribute("href")!.slice(1));
    expect(target).toHaveAttribute("data-owner", "queen");
    expect(target).toHaveAttribute("tabindex", "-1");
    expect(within(target!).getByRole("heading", { name: "Waiting on Queen 1" })).toBeVisible();
    expect(within(index).getByRole("link", { name: "Dependencies / holds 1" })).toBeVisible();
    expect(within(index).queryByRole("link", { name: /You/ })).not.toBeInTheDocument();
    rerender(<QueuesView tasks={[waiting[1]]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.queryByRole("link", { name: "Queen 1" })).not.toBeInTheDocument();
    expect(onOpenTask).not.toHaveBeenCalled();
  });
});
