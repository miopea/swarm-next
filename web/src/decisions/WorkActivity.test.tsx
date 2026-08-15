import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Task, TaskActivityPage } from "../api";
import WorkActivity from "./WorkActivity";

afterEach(cleanup);

const tasks = [
  { id: "task-1", title: "Ship the garden" },
  { id: "task-2", title: "Tend the worker" },
] as Task[];
const workers = [{ id: "worker-1", name: "Daisy" }] as import("../api").Worker[];
const activity: TaskActivityPage = {
  truncated: false,
  events: [
    { sequence: 1, task_id: "task-1", kind: "created", from_state: null, to_state: "draft", note: "", occurred_at: 1, actor_kind: "operator", actor_id: null },
    { sequence: 2, task_id: "task-1", kind: "state_changed", from_state: "ready", to_state: "active", note: "Started safely", occurred_at: 2, actor_kind: "worker", actor_id: "worker-1" },
    { sequence: 3, task_id: "task-2", kind: "assigned", from_state: null, to_state: null, note: "", occurred_at: 3, actor_kind: "jira", actor_id: null },
  ],
};

test("filters durable activity and opens its task", () => {
  const onOpenTask = vi.fn();
  const onRetry = vi.fn();
  render(<WorkActivity activity={activity} tasks={tasks} workers={workers} loading={false} failed={false} onRetry={onRetry} onOpenTask={onOpenTask} />);

  expect(screen.getByText("Started safely")).toBeInTheDocument();
  expect(screen.getByText("Daisy")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Show"), { target: { value: "assignments" } });
  expect(screen.getByText("Worker assigned")).toBeInTheDocument();
  expect(screen.queryByText("Started safely")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Tend the worker" }));
  expect(onOpenTask).toHaveBeenCalledWith("task-2");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  expect(onRetry).toHaveBeenCalled();
});

test("searches by task title without exposing transport events", () => {
  render(<WorkActivity activity={activity} tasks={tasks} workers={workers} loading={false} failed={false} onRetry={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("Find work"), { target: { value: "garden" } });
  expect(screen.getAllByText("Ship the garden")).toHaveLength(2);
  expect(screen.queryByText("Tend the worker")).not.toBeInTheDocument();
});

test("filters activity by its durable source", () => {
  render(<WorkActivity activity={activity} tasks={tasks} workers={workers} loading={false} failed={false} onRetry={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("Source"), { target: { value: "operator" } });
  const actors = [...document.querySelectorAll(".activity-actor")].map((entry) => entry.textContent);
  expect(actors).toEqual(["You"]);
});
