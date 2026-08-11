import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { Task } from "../api";
import TaskBoard from "./TaskBoard";

const task: Task = {
  id: "task-1", title: "Make reload stable", workspace: "/workspace/swarm", state: "draft",
  assigned_session_id: null, created_at: 1, updated_at: 1,
};

test("dragging a task exposes only legal workflow targets and performs the drop", () => {
  const onTransition = vi.fn().mockResolvedValue(undefined);
  render(<TaskBoard tasks={[task]} sessions={[]} workerNames={new Map()} busy={false} onCreate={vi.fn()} onTransition={onTransition} onAssign={vi.fn()} onStartWorker={vi.fn()} />);
  const dataTransfer = { effectAllowed: "none", setData: vi.fn() };

  fireEvent.dragStart(screen.getByRole("article", { name: task.title }), { dataTransfer });

  expect(screen.getByText(task.title, { selector: ".task-drop-strip strong" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Ready" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "In progress" })).not.toBeInTheDocument();

  fireEvent.drop(screen.getByRole("button", { name: "Ready" }), { dataTransfer });
  expect(onTransition).toHaveBeenCalledWith(task, "ready");
  expect(screen.queryByText(task.title, { selector: ".task-drop-strip strong" })).not.toBeInTheDocument();
});
