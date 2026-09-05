import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import QueuesView from "./QueuesView";
import type { Task } from "../api/tasks";

function task(overrides: Partial<Task>): Task {
  return {
    id: "t1", hive_id: "h", title: "Some work", description: "", operator_instruction: "",
    workspace: "/w", state: "review", priority: "normal", assigned_worker_id: null,
    assigned_session_id: null, position: 0, created_at: 1, updated_at: 1,
    ...overrides,
  } as Task;
}

describe("QueuesView", () => {
  test("explicit prerequisites link to their task and completion waits for Queen", () => {
    const prerequisite = { task_id: "t1", prerequisite_id: "upstream", title: "Shared API contract", state: "active" as const, assigned_worker_id: null, removed: false, reason: "The response shape must be settled first", created_at: 1 };
    const blocked = task({ state: "blocked", next_move_owner: "blocked", prerequisites: [prerequisite] });
    const onOpenTask = vi.fn();
    const { rerender } = render(<QueuesView tasks={[blocked]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.getByText("1 unresolved prerequisite")).toBeVisible();
    expect(screen.getByText(/No worker assigned/)).toBeVisible();
    expect(screen.queryByText("Blocked · reason not recorded")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: prerequisite.title }));
    expect(onOpenTask).toHaveBeenCalledWith("upstream");
    rerender(<QueuesView tasks={[{ ...blocked, next_move_owner: "queen", prerequisites: [{ ...prerequisite, state: "completed" }] }]} workers={[]} onOpenTask={onOpenTask} />);
    expect(screen.getByText("Prerequisites completed · Queen checks remaining blockers before resuming")).toBeVisible();
    expect(screen.getByText("1 completed prerequisite").closest("details")).not.toHaveAttribute("open");
    expect(screen.queryByText("1 unresolved prerequisite")).not.toBeInTheDocument();
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
    expect(screen.getByText("Briefing held: behind Earlier task · queued under a minute")).toBeVisible();
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
    expect(details?.querySelector("summary")).toHaveTextContent(`Recorded when blocked: ${note.slice(0, 240)}…`);
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
    rerender(<QueuesView {...props} heldDeliveries={[]} />);
    expect(screen.queryByText("Queen: reconcile message delivery")).not.toBeInTheDocument();
  });
  test("retains delivery evidence without claiming the Queen has stopped, then clears it", () => {
    const props = { tasks: [], workers: [], onOpenTask: vi.fn() };
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
      task({ id: "e", title: "Needs a ruling", next_move_owner: "operator" }),
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
    render(<QueuesView onOpenTask={vi.fn()} workers={[]} tasks={[
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
});
