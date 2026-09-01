import { render, screen } from "@testing-library/react";
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
    ]} />);

    expect(screen.getByRole("heading", { name: /Waiting on Queen 2/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Waiting on a worker 1/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Blocked on something else 1/ })).toBeInTheDocument();
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
    expect(screen.getByText("Nothing is waiting on anyone.")).toBeInTheDocument();
  });
});
