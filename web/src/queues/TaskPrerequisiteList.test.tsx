import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Task, TaskPrerequisite } from "../api/tasks";
import TaskPrerequisiteList from "./TaskPrerequisiteList";

afterEach(cleanup);

test("mixed dependencies expose live blockers and fold completed work, including after reopening", () => {
  const dependency = (id: string, state: TaskPrerequisite["state"], removed = false): TaskPrerequisite => ({
    task_id: "consumer", prerequisite_id: id, title: id, state, removed,
    assigned_worker_id: null, reason: "Required contract", created_at: 1,
  });
  const prerequisites = [dependency("Finished contract", "completed"), dependency("Pending verification", "review"), dependency("Removed contract", "completed", true)];
  const task = { state: "blocked", next_move_owner: "blocked", prerequisites } as Task;
  const onOpenTask = vi.fn();
  const props = { workerNames: new Map<string, string>(), onOpenTask, compact: true };
  const { rerender } = render(<TaskPrerequisiteList {...props} task={task} />);
  expect(screen.getByText("2 unresolved prerequisites")).toBeVisible();
  expect(screen.getByText("Pending verification")).toBeVisible();
  expect(screen.getByText("Removed contract")).toBeVisible();
  expect(screen.getByText("Finished contract")).not.toBeVisible();
  fireEvent.click(screen.getByText("1 completed prerequisite"));
  fireEvent.click(screen.getByRole("button", { name: "Finished contract" }));
  expect(onOpenTask).toHaveBeenCalledExactlyOnceWith("Finished contract");
  rerender(<TaskPrerequisiteList {...props} task={{ ...task, prerequisites: [dependency("Finished contract", "active"), ...prerequisites.slice(1)] }} />);
  expect(screen.getByText("3 unresolved prerequisites")).toBeVisible();
  expect(screen.getByRole("button", { name: "Finished contract" })).toBeVisible();
  expect(screen.queryByText("1 completed prerequisite")).not.toBeInTheDocument();
});
